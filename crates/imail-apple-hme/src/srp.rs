use num_bigint::BigUint;
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::error::{AppleHmeError, AppleHmeErrorCode, Result};

const N_HEX: &str = concat!(
    "AC6BDB41324A9A9BF166DE5E1389582FAF72B6651987EE07FC3192943DB56050",
    "A37329CBB4A099ED8193E0757767A13DD52312AB4B03310DCD7F48A9DA04FD50",
    "E8083969EDB767B0CF6095179A163AB3661A05FBD5FAAAE82918A9962F0B93B8",
    "55F97993EC975EEAA80D740ADBF4FF747359D041D5C33EA71D281E446B14773B",
    "CA97B43A23FB801676BD207A436C6481F1D2B9078717461A5B9D32E688F87748",
    "544523B524B0D57D5EA77A2775D2ECFA032CFBDBF52FB3786160279004E57AE",
    "6AF874E7303CE53299CCC041C7BC308D82A5698F3A8D0C38271AE35F8E9DBFB",
    "B694B5C803D89F7AE435DE236D525F54759B65E372FCD68EF20FA7111F9E4AFF73"
);

const PAD_BYTES: usize = 256;

pub(crate) struct SrpClient {
    secret: Vec<u8>,
    a: BigUint,
    public_a: BigUint,
}

#[derive(Debug)]
pub(crate) struct SrpProof {
    pub m1: Vec<u8>,
    pub m2: Vec<u8>,
}

impl Drop for SrpClient {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

impl SrpClient {
    pub fn random() -> Result<Self> {
        let mut secret = vec![0_u8; 32];
        rand::thread_rng().fill_bytes(&mut secret);
        Self::from_secret(secret)
    }

    fn from_secret(secret: Vec<u8>) -> Result<Self> {
        let n = modulus()?;
        let a = BigUint::from_bytes_be(&secret);
        if a == BigUint::default() {
            return Err(AppleHmeError::new(
                AppleHmeErrorCode::Protocol,
                "SRP 随机数无效",
                false,
            ));
        }
        let public_a = BigUint::from(2_u8).modpow(&a, &n);
        Ok(Self {
            secret,
            a,
            public_a,
        })
    }

    pub fn public_a(&self) -> Vec<u8> {
        pad(&self.public_a)
    }

    pub fn prove(
        &self,
        username: &[u8],
        password: &str,
        protocol: &str,
        iterations: u32,
        salt: &[u8],
        server_b: &[u8],
    ) -> Result<SrpProof> {
        if iterations == 0 {
            return Err(AppleHmeError::bad_response("Apple SRP iteration 无效"));
        }
        let n = modulus()?;
        let g = BigUint::from(2_u8);
        let b = BigUint::from_bytes_be(server_b);
        if b == BigUint::default() || b >= n {
            return Err(AppleHmeError::bad_response("Apple SRP B 参数无效"));
        }

        let mut password_hash = Sha256::digest(password.as_bytes()).to_vec();
        let mut password_input = match protocol {
            "s2k" => password_hash.clone(),
            "s2k_fo" => hex_lower(&password_hash).into_bytes(),
            value => {
                password_hash.zeroize();
                return Err(AppleHmeError::new(
                    AppleHmeErrorCode::Unsupported,
                    format!("不支持 Apple SRP 协议 {value}"),
                    false,
                ));
            }
        };
        password_hash.zeroize();
        let mut derived = vec![0_u8; 32];
        pbkdf2_hmac::<Sha256>(&password_input, salt, iterations, &mut derived);
        password_input.zeroize();

        let mut inner = Sha256::new();
        inner.update(b":");
        inner.update(&derived);
        let inner = inner.finalize();
        derived.zeroize();
        let mut x_hash = Sha256::new();
        x_hash.update(salt);
        x_hash.update(inner);
        let x = BigUint::from_bytes_be(&x_hash.finalize());

        let mut u_hash = Sha256::new();
        u_hash.update(pad(&self.public_a));
        u_hash.update(pad(&b));
        let u = BigUint::from_bytes_be(&u_hash.finalize());
        if u == BigUint::default() {
            return Err(AppleHmeError::bad_response("Apple SRP u 参数无效"));
        }

        let mut k_hash = Sha256::new();
        k_hash.update(n.to_bytes_be());
        k_hash.update(pad(&g));
        let k = BigUint::from_bytes_be(&k_hash.finalize());
        let gx = g.modpow(&x, &n);
        let kgx = (&k * gx) % &n;
        let base = if b >= kgx {
            &b - &kgx
        } else {
            (&b + &n) - &kgx
        };
        let exponent = &self.a + (&u * &x);
        let shared = base.modpow(&exponent, &n);
        let key = Sha256::digest(pad(&shared));

        let h_g = Sha256::digest(pad(&g));
        let h_n = Sha256::digest(n.to_bytes_be());
        let xor = h_g
            .iter()
            .zip(h_n.iter())
            .map(|(left, right)| left ^ right)
            .collect::<Vec<_>>();
        let mut m1_hash = Sha256::new();
        m1_hash.update(xor);
        m1_hash.update(Sha256::digest(username));
        m1_hash.update(salt);
        let public_a = pad(&self.public_a);
        m1_hash.update(&public_a);
        m1_hash.update(pad(&b));
        m1_hash.update(key);
        let m1 = m1_hash.finalize().to_vec();

        let mut m2_hash = Sha256::new();
        m2_hash.update(&public_a);
        m2_hash.update(&m1);
        m2_hash.update(key);
        let m2 = m2_hash.finalize().to_vec();
        Ok(SrpProof { m1, m2 })
    }
}

fn modulus() -> Result<BigUint> {
    BigUint::parse_bytes(N_HEX.as_bytes(), 16)
        .ok_or_else(|| AppleHmeError::new(AppleHmeErrorCode::Protocol, "Apple SRP 模数无效", false))
}

fn pad(value: &BigUint) -> Vec<u8> {
    let bytes = value.to_bytes_be();
    if bytes.len() >= PAD_BYTES {
        return bytes;
    }
    let mut padded = vec![0_u8; PAD_BYTES];
    padded[PAD_BYTES - bytes.len()..].copy_from_slice(&bytes);
    padded
}

fn hex_lower(value: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(value.len() * 2);
    for byte in value {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_fixed_width_public_value_and_deterministic_proofs() {
        let client = SrpClient::from_secret(vec![7; 32]).unwrap();
        assert_eq!(client.public_a().len(), PAD_BYTES);
        let server_b = BigUint::from(2_u8)
            .modpow(&BigUint::from(99_u8), &modulus().unwrap())
            .to_bytes_be();
        let first = client
            .prove(
                b"owner@example.com",
                "password",
                "s2k",
                1000,
                b"salt",
                &server_b,
            )
            .unwrap();
        let second = client
            .prove(
                b"owner@example.com",
                "password",
                "s2k",
                1000,
                b"salt",
                &server_b,
            )
            .unwrap();
        assert_eq!(first.m1, second.m1);
        assert_eq!(first.m2, second.m2);
        assert_eq!(client.public_a().len(), PAD_BYTES);
    }

    #[test]
    fn rejects_unknown_apple_password_protocol() {
        let client = SrpClient::from_secret(vec![3; 32]).unwrap();
        let error = client
            .prove(b"owner", "password", "future", 1, b"salt", &[2])
            .unwrap_err();
        assert_eq!(error.code, AppleHmeErrorCode::Unsupported);
    }
}

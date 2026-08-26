import { describe, expect, it } from 'vitest';
import { findVerificationCode } from './verification-code';

describe('verification code detection', () => {
  it('detects a code before an English sign-in label', () => {
    expect(findVerificationCode('486405 is your Kickstarter sign-in code. This verification code expires in 10 minutes.')).toBe('486405');
  });

  it('detects common Chinese and OTP formats', () => {
    expect(findVerificationCode('您的登录验证码为 839271，请勿告知他人。')).toBe('839271');
    expect(findVerificationCode('Your OTP: 7284. It expires shortly.')).toBe('7284');
  });

  it('does not treat unrelated numbers as verification codes', () => {
    expect(findVerificationCode('订单 486405 已发货，预计 2026 年送达。')).toBeUndefined();
    expect(findVerificationCode('The verification email was sent and expires in 10 minutes.')).toBeUndefined();
  });

  it('prefers the candidate closest to the verification context', () => {
    expect(findVerificationCode('Reference 12345678. Your security code is 654321 and expires in 10 minutes.')).toBe('654321');
  });
});

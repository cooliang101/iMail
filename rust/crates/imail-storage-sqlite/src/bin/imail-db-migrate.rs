use std::{env, process};

use imail_storage_sqlite::migrate_database;

fn main() {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 1 {
        eprintln!("用法：imail-db-migrate <database>");
        process::exit(2);
    }
    match migrate_database(&arguments[0]) {
        Ok(report) => println!(
            "{}",
            serde_json::to_string(&report).expect("serialize migration report")
        ),
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    }
}

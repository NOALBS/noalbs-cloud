//! Migrations that ran in production must never change: sqlx refuses to start
//! on a database whose applied migration no longer matches ("was previously
//! applied but has been modified"), which would stop the deploy.
//!
//! When a new migration is released, add its version and checksum here (this
//! test prints them when one is missing from the list).

use std::fmt::Write;

/// (version, SHA-384 of the SQL as sqlx computes it), for every released migration.
const RELEASED: &[(i64, &str)] = &[
    (
        20260806085445,
        "f7776a21e9b4cc38eb9c150d3f4aaec265269b2f397bc6acc83dc98a76198afb8cf2ff33d55fed6b154f18bf56f39a6f",
    ), // initial (first production release)
    (
        20260926120000,
        "45c297517db798a1bbb07b480eaed3187b828a86bd4df8fc682d7945d6b4ba6010b02f2b7de20774b7c814cb969076c7",
    ), // chat link notifications
    (
        20260926155837,
        "2d74c483ca428e37bfe75b4158fd41bb8ce013924682f7ab65ba6c0fc07b7ee089759f0904bbe09ef0a4d242aa0fba02",
    ), // more chat permissions
    (
        20260926160000,
        "6de2d06b1b26d2a089f936f02d81b14475dc104fa9c9e3073e4e38983e4b8a3e1cf452fc773a3663e9c6251f8d030b68",
    ), // privacy
    (
        20260926180000,
        "38d850456eb8bbd9eaa780345f54f97c7ccc115f2e996ed12552d157346b073047a5adedae46186f00f95fb074579abb",
    ), // default chat aliases
    (
        20260927120000,
        "395b26c3b02c458bbe0dfc609c03df75d9b0d597a36f7c079199a91b79b3dd9323c586145ae2410c441c4a8ccc2c4dee",
    ), // threaded replies
    (
        20260927150000,
        "49c810eed727770dc2ce06096a3babd0bcb2d72110fa59ee066af21e6bdf5d2b780900c3205e2dc00adb8633b8a2d5f1",
    ), // belabox
];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[test]
fn released_migrations_are_unchanged() {
    let migrations = db::MIGRATOR.iter().collect::<Vec<_>>();
    for (version, checksum) in RELEASED {
        let migration = migrations
            .iter()
            .find(|migration| migration.version == *version)
            .unwrap_or_else(|| panic!("released migration {version} is missing"));
        assert_eq!(
            hex(&migration.checksum),
            *checksum,
            "released migration {version} ({}) was edited; add a new migration instead",
            migration.description
        );
    }
    for migration in &migrations {
        if !RELEASED
            .iter()
            .any(|(version, _)| *version == migration.version)
        {
            println!(
                "not pinned yet: ({}, \"{}\"), // {}",
                migration.version,
                hex(&migration.checksum),
                migration.description
            );
        }
    }
}

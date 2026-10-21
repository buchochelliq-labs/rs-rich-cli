//! The fixture marketplace, end to end: signed with the test keys in
//! `tests/fixtures/keys` (see `support/mod.rs` for how they were made).

mod support;

use std::fs;

use rich_marketplace::semver::Version;
use rich_marketplace::{
    resolve, verify_index, AcceptedIndex, CheckError, FetchError, Host, Incompatibility,
    IndexError, InstallError, Installer, KeyChainError, KeySet, Kind, ListingError, Lockfile,
    LockfileError, MemoryTransport, Request, ResolveError, Rotation, RotationKind, SignatureError,
    VerifiedIndex, LOCKFILE_NAME,
};
use support::*;

const SERIAL: u64 = 42;

fn verified(market: &Marketplace) -> VerifiedIndex {
    let index = market.index(SERIAL);
    let signature = sign_index(&pair("gen1-main"), &index, SERIAL);
    verify_index(&generation_1(), &index, &signature, None, now())
        .expect("the fixture index verifies")
}

fn exact(version: &str) -> Request {
    Request::Exact(Version::parse(version).unwrap())
}

#[test]
#[ignore = "writes new test keys; run by hand to replace them"]
fn generate_test_keys() {
    let dir = keys_dir();
    fs::create_dir_all(&dir).unwrap();
    for name in KEY_NAMES {
        let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let public = pair.pk.to_box().unwrap().into_string();
        let secret = pair
            .sk
            .to_box(Some("rs-rich-marketplace TEST key: never trust it"))
            .unwrap()
            .into_string();
        fs::write(dir.join(format!("{name}.pub")), public).unwrap();
        fs::write(dir.join(format!("{name}.key")), secret).unwrap();
    }
}

#[test]
fn a_good_install_resolves_downloads_checks_and_locks() {
    let market = Marketplace::build();
    let index = verified(&market);
    assert_eq!(index.serial(), SERIAL);
    assert_eq!(index.listings().len(), 4);

    let listing = index
        .read_listing("shout", market.listing("shout"))
        .unwrap();
    // 0.4.0 has a capability kind this host does not know, 0.3.0 needs ABI 2
    // and 0.2.1 is yanked: 0.2.0 is the newest candidate.
    let version = resolve(&listing, &Host::current(), &Request::Newest).unwrap();
    assert_eq!(version.version.to_string(), "0.2.0");

    let root = temp_dir("good");
    let installer = Installer::new(&root, market.transport.clone());
    let entry = installer.install(&index, &listing, version, now()).unwrap();
    assert_eq!(entry.path, "shout/shout-0.2.0.wasm");
    assert_eq!(entry.index_serial, SERIAL);
    assert_eq!(installer.load("shout").unwrap(), shout_wasm("0.2.0"));

    // The lockfile records it, and reads back the same.
    let lock = Lockfile::load(&root.join(LOCKFILE_NAME)).unwrap();
    assert_eq!(lock.get("shout"), Some(&entry));
    assert!(lock.verify_all(&root).iter().all(|(_, r)| r.is_ok()));

    // A micro pack and a theme install through their own checks.
    for id in ["status", "solarized"] {
        let listing = index.read_listing(id, market.listing(id)).unwrap();
        let version = resolve(&listing, &Host::current(), &Request::Newest).unwrap();
        installer.install(&index, &listing, version, now()).unwrap();
        installer.load(id).unwrap();
    }
    assert_eq!(installer.lockfile().unwrap().entries().count(), 3);

    // Nothing is left behind but the files and the lockfile.
    let mut names: Vec<_> = fs::read_dir(root.join("shout"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["shout-0.2.0.wasm"]);

    installer.remove("shout").unwrap();
    assert!(!root.join("shout").exists());
    assert!(matches!(
        installer.load("shout"),
        Err(LockfileError::NotInstalled(_))
    ));
}

#[test]
fn a_bad_hash_installs_nothing() {
    let market = Marketplace::build();
    let index = verified(&market);
    let listing = index
        .read_listing("shout", market.listing("shout"))
        .unwrap();
    let version = resolve(&listing, &Host::current(), &exact("0.2.0")).unwrap();

    // The same size, other bytes.
    let mut body = shout_wasm("0.2.0");
    *body.last_mut().unwrap() ^= 1;
    let transport = MemoryTransport::new().with(version.artifact.url.clone(), body);
    let root = temp_dir("bad-hash");
    let error = Installer::new(&root, transport)
        .install(&index, &listing, version, now())
        .unwrap_err();
    assert!(
        matches!(error, InstallError::HashMismatch { .. }),
        "{error}"
    );
    assert!(!root.join(LOCKFILE_NAME).exists());
    assert!(!root.join("shout").exists());
}

#[test]
fn the_declared_size_bounds_the_download() {
    let market = Marketplace::build();
    let index = verified(&market);
    let listing = index
        .read_listing("shout", market.listing("shout"))
        .unwrap();
    let version = resolve(&listing, &Host::current(), &exact("0.2.0")).unwrap();
    let url = version.artifact.url.clone();
    let root = temp_dir("size");

    let longer = [shout_wasm("0.2.0"), vec![0]].concat();
    let error = Installer::new(&root, MemoryTransport::new().with(url.clone(), longer))
        .install(&index, &listing, version, now())
        .unwrap_err();
    assert!(
        matches!(error, InstallError::Fetch(FetchError::TooLarge { .. })),
        "{error}"
    );

    let mut shorter = shout_wasm("0.2.0");
    shorter.pop();
    let error = Installer::new(&root, MemoryTransport::new().with(url, shorter))
        .install(&index, &listing, version, now())
        .unwrap_err();
    assert!(
        matches!(error, InstallError::SizeMismatch { .. }),
        "{error}"
    );
    assert!(!root.join(LOCKFILE_NAME).exists());
}

#[test]
fn a_bad_signature_is_refused() {
    let market = Marketplace::build();
    let index = market.index(SERIAL);
    let signature = sign_index(&pair("gen1-main"), &index, SERIAL);
    // One byte of the index changed after signing.
    let mut tampered = index.clone();
    let at = tampered.iter().position(|&b| b == b'5').unwrap();
    tampered[at] = b'6';
    let error = verify_index(&generation_1(), &tampered, &signature, None, now()).unwrap_err();
    assert_eq!(error, IndexError::Signature(SignatureError::Invalid));

    // A trusted comment edited after signing (the signature covers it).
    let edited = signature.replace("serial=42", "serial=43");
    let error = verify_index(&generation_1(), &index, &edited, None, now()).unwrap_err();
    assert_eq!(error, IndexError::Signature(SignatureError::Invalid));

    // A signature for another serial cannot be moved to this index.
    let other = sign_index(&pair("gen1-main"), &index, SERIAL + 1);
    let error = verify_index(&generation_1(), &index, &other, None, now()).unwrap_err();
    assert_eq!(
        error,
        IndexError::SerialMismatch {
            signed: SERIAL + 1,
            index: SERIAL
        }
    );

    // A comment that is not the index's.
    let wrong = pair("gen1-main").sign(&index, "timestamp:1792540800");
    let error = verify_index(&generation_1(), &index, &wrong, None, now()).unwrap_err();
    assert!(matches!(error, IndexError::TrustedComment(_)), "{error}");

    assert_eq!(
        verify_index(&generation_1(), &index, "not a signature", None, now()).unwrap_err(),
        IndexError::Signature(SignatureError::Malformed)
    );
}

#[test]
fn a_wrong_key_id_is_refused_before_checking() {
    let market = Marketplace::build();
    let index = market.index(SERIAL);
    let main = pair("gen1-main").trusted.id();
    for signer in ["stranger", "gen1-backup"] {
        let key = pair(signer);
        let signature = sign_index(&key, &index, SERIAL);
        let error = verify_index(&generation_1(), &index, &signature, None, now()).unwrap_err();
        assert_eq!(
            error,
            IndexError::Signature(SignatureError::WrongKeyId {
                expected: main,
                found: key.trusted.id()
            }),
            "signed by {signer}"
        );
    }
}

#[test]
fn a_rolled_back_serial_is_refused() {
    let market = Marketplace::build();
    let main = pair("gen1-main");
    let newer = market.index(SERIAL);
    let accepted = verify_index(
        &generation_1(),
        &newer,
        &sign_index(&main, &newer, SERIAL),
        None,
        now(),
    )
    .unwrap()
    .accepted();

    let older = market.index(SERIAL - 1);
    let error = verify_index(
        &generation_1(),
        &older,
        &sign_index(&main, &older, SERIAL - 1),
        Some(&accepted),
        now(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        IndexError::Rollback {
            serial: SERIAL - 1,
            last: SERIAL
        }
    );

    // The same serial again is fine with the same bytes, not with others.
    let again = verify_index(
        &generation_1(),
        &newer,
        &sign_index(&main, &newer, SERIAL),
        Some(&accepted),
        now(),
    );
    assert!(again.is_ok());
    let other = AcceptedIndex {
        serial: SERIAL,
        sha256: "0".repeat(64),
    };
    let error = verify_index(
        &generation_1(),
        &newer,
        &sign_index(&main, &newer, SERIAL),
        Some(&other),
        now(),
    )
    .unwrap_err();
    assert_eq!(error, IndexError::SerialReused { serial: SERIAL });
}

#[test]
fn an_expired_index_is_refused_for_resolving_and_installing() {
    let market = Marketplace::build();
    let index = market.index(SERIAL);
    let signature = sign_index(&pair("gen1-main"), &index, SERIAL);
    let error =
        verify_index(&generation_1(), &index, &signature, None, after_expiry()).unwrap_err();
    assert_eq!(
        error,
        IndexError::Expired {
            expires: "2026-11-01T00:00:00Z".into()
        }
    );

    // An index verified while fresh and cached is refused for an install
    // once it expires, but what is installed still loads.
    let index = verified(&market);
    let listing = index
        .read_listing("solarized", market.listing("solarized"))
        .unwrap();
    let version = &listing.versions[0];
    let root = temp_dir("expired");
    let installer = Installer::new(&root, market.transport.clone());
    installer.install(&index, &listing, version, now()).unwrap();
    let error = installer
        .install(&index, &listing, version, after_expiry())
        .unwrap_err();
    assert!(
        matches!(error, InstallError::Index(IndexError::Expired { .. })),
        "{error}"
    );
    assert_eq!(installer.load("solarized").unwrap(), THEME);
}

#[test]
fn a_yanked_version_is_never_chosen_or_installed() {
    let market = Marketplace::build();
    let index = verified(&market);
    let listing = index
        .read_listing("shout", market.listing("shout"))
        .unwrap();
    let error = resolve(&listing, &Host::current(), &exact("0.2.1")).unwrap_err();
    assert_eq!(
        error,
        ResolveError::Yanked {
            id: "shout".into(),
            version: "0.2.1".into(),
            reason: "Drops combining marks; use 0.2.0.".into()
        }
    );
    let yanked = listing.version(&Version::parse("0.2.1").unwrap()).unwrap();
    let root = temp_dir("yanked");
    let error = Installer::new(&root, market.transport.clone())
        .install(&index, &listing, yanked, now())
        .unwrap_err();
    assert!(
        matches!(error, InstallError::Resolve(ResolveError::Yanked { .. })),
        "{error}"
    );
    assert!(matches!(
        resolve(&listing, &Host::current(), &exact("9.9.9")),
        Err(ResolveError::NoSuchVersion { .. })
    ));
}

#[test]
fn an_abi_the_host_cannot_load_is_skipped_or_refused() {
    let market = Marketplace::build();
    let index = verified(&market);
    let listing = index
        .read_listing("shout", market.listing("shout"))
        .unwrap();
    let host = Host::current();

    let error = resolve(&listing, &host, &exact("0.3.0")).unwrap_err();
    assert_eq!(
        error,
        ResolveError::Incompatible {
            id: "shout".into(),
            version: "0.3.0".into(),
            reason: Incompatibility::Abi {
                required: (2, 0),
                host: (1, 0)
            }
        }
    );
    let error = resolve(&listing, &host, &exact("0.4.0")).unwrap_err();
    assert!(
        matches!(
            &error,
            ResolveError::Incompatible {
                reason: Incompatibility::UnknownCapability { kind },
                ..
            } if kind == "fence-svg"
        ),
        "{error}"
    );

    // Installing it anyway is refused before any download.
    let v030 = listing.version(&Version::parse("0.3.0").unwrap()).unwrap();
    let root = temp_dir("abi");
    let error = Installer::new(&root, MemoryTransport::new())
        .install(&index, &listing, v030, now())
        .unwrap_err();
    assert!(
        matches!(
            error,
            InstallError::Resolve(ResolveError::Incompatible { .. })
        ),
        "{error}"
    );

    // A host of ABI 2 takes 0.3.0, the newest version for its major.
    let mut abi2 = host;
    abi2.abi_major = 2;
    let chosen = resolve(&listing, &abi2, &Request::Newest).unwrap();
    assert_eq!(chosen.version.to_string(), "0.3.0");

    // A host older than every version is told a newer host is needed.
    let mut abi0 = host;
    abi0.abi_major = 0;
    assert_eq!(
        resolve(&listing, &abi0, &Request::Newest).unwrap_err(),
        ResolveError::NoCandidate {
            id: "shout".into(),
            newer_host_needed: Some("0.4.0".into())
        }
    );
}

#[test]
fn a_crate_is_listed_never_installed() {
    let market = Marketplace::build();
    let index = verified(&market);
    let listing = index
        .read_listing("mermaid", market.listing("mermaid"))
        .unwrap();
    assert_eq!(listing.kind, Kind::Crate);
    assert_eq!(listing.crate_info.as_ref().unwrap().name, "rs-rich-mermaid");
    let version = resolve(&listing, &Host::current(), &Request::Newest).unwrap();
    let error = Installer::new(temp_dir("crate"), MemoryTransport::new())
        .install(&index, &listing, version, now())
        .unwrap_err();
    assert!(
        matches!(error, InstallError::NotInstallable { kind: "crate", .. }),
        "{error}"
    );
}

#[test]
fn listings_are_checked_against_the_index() {
    let market = Marketplace::build();
    let index = verified(&market);
    let mut changed = market.listing("shout").to_vec();
    changed.push(b'\n');
    assert!(matches!(
        index.read_listing("shout", &changed),
        Err(ListingError::HashMismatch { .. })
    ));
    assert!(matches!(
        index.read_listing("status", market.listing("shout")),
        Err(ListingError::HashMismatch { .. })
    ));
    assert_eq!(
        index.read_listing("nothing", b"{}").unwrap_err(),
        ListingError::NotInIndex("nothing".into())
    );
}

#[test]
fn artifacts_are_checked_by_kind() {
    let market = Marketplace::build();
    let index = verified(&market);
    let root = temp_dir("kind");

    // A micro pack whose pack.json names another pack. Its listing is
    // rebuilt so the hash and size match; the check by kind still refuses.
    let other = pack_zip("other", "1.0.0");
    let listing = index
        .read_listing("status", market.listing("status"))
        .unwrap();
    let mut version = listing.versions[0].clone();
    version.artifact.sha256 = sha256(&other);
    version.artifact.size = other.len() as u64;
    let transport = MemoryTransport::new().with(version.artifact.url.clone(), other);
    let error = Installer::new(&root, transport)
        .install(&index, &listing, &version, now())
        .unwrap_err();
    assert!(
        matches!(
            error,
            InstallError::Check(CheckError::ManifestMismatch {
                field: "pack name",
                ..
            })
        ),
        "{error}"
    );

    // A theme that does not parse.
    let listing = index
        .read_listing("solarized", market.listing("solarized"))
        .unwrap();
    let mut version = listing.versions[0].clone();
    let broken = b"no styles section\n".to_vec();
    version.artifact.sha256 = sha256(&broken);
    version.artifact.size = broken.len() as u64;
    let transport = MemoryTransport::new().with(version.artifact.url.clone(), broken);
    let error = Installer::new(&root, transport)
        .install(&index, &listing, &version, now())
        .unwrap_err();
    assert!(
        matches!(error, InstallError::Check(CheckError::Theme(_))),
        "{error}"
    );

    // A host's own WASM check (the sandbox, in the CLI) replaces the header check.
    struct Refuse;
    impl rich_marketplace::ArtifactChecker for Refuse {
        fn check_wasm(
            &self,
            _: &[u8],
            expected: &rich_marketplace::WasmExpectation<'_>,
        ) -> Result<(), CheckError> {
            Err(CheckError::Host(format!(
                "{} refused by the host",
                expected.id
            )))
        }
    }
    let listing = index
        .read_listing("shout", market.listing("shout"))
        .unwrap();
    let version = resolve(&listing, &Host::current(), &Request::Newest).unwrap();
    let error = Installer::new(&root, market.transport.clone())
        .with_checker(Refuse)
        .install(&index, &listing, version, now())
        .unwrap_err();
    assert!(
        matches!(error, InstallError::Check(CheckError::Host(_))),
        "{error}"
    );
    assert!(!root.join(LOCKFILE_NAME).exists());
}

#[test]
fn a_file_changed_on_disk_does_not_load() {
    let market = Marketplace::build();
    let index = verified(&market);
    let listing = index
        .read_listing("shout", market.listing("shout"))
        .unwrap();
    let version = resolve(&listing, &Host::current(), &Request::Newest).unwrap();
    let root = temp_dir("lock");
    let installer = Installer::new(&root, market.transport.clone());
    let entry = installer.install(&index, &listing, version, now()).unwrap();

    let file = entry.file(&root);
    let mut bytes = fs::read(&file).unwrap();
    bytes[9] ^= 0xff;
    fs::write(&file, &bytes).unwrap();
    let error = installer.load("shout").unwrap_err();
    assert!(matches!(error, LockfileError::Mismatch { .. }), "{error}");
    let report = installer.lockfile().unwrap().verify_all(&root);
    assert!(matches!(report[0].1, Err(LockfileError::Mismatch { .. })));

    fs::remove_file(&file).unwrap();
    assert!(matches!(
        installer.load("shout"),
        Err(LockfileError::Missing { .. })
    ));
}

#[test]
fn a_planned_rotation_is_signed_by_both_keys() {
    let (main1, backup1, main2, backup2) = (
        pair("gen1-main"),
        pair("gen1-backup"),
        pair("gen2-main"),
        pair("gen2-backup"),
    );
    let next = keys_json(2, &main2, &backup2, &[]);
    let comment = "rs-rich-marketplace keys generation=2";
    let (main_sig, backup_sig) = (main1.sign(&next, comment), backup1.sign(&next, comment));
    let (keys, kinds) = generation_1()
        .walk([Rotation {
            keys_json: &next,
            main_signature: Some(&main_sig),
            backup_signature: &backup_sig,
        }])
        .unwrap();
    assert_eq!(kinds, [RotationKind::Planned]);
    assert_eq!(keys.generation(), 2);
    assert_eq!(keys.main().id(), main2.trusted.id());
    // It round-trips, so a client can store the newest generation.
    assert_eq!(KeySet::from_json(keys.to_json().as_bytes()).unwrap(), keys);

    // The index now verifies with the new main key only.
    let market = Marketplace::build();
    let index = market.index(SERIAL);
    assert!(verify_index(
        &keys,
        &index,
        &sign_index(&main2, &index, SERIAL),
        None,
        now()
    )
    .is_ok());
    let old = verify_index(
        &keys,
        &index,
        &sign_index(&main1, &index, SERIAL),
        None,
        now(),
    );
    assert!(matches!(
        old,
        Err(IndexError::Signature(SignatureError::WrongKeyId { .. }))
    ));

    // Signed by the main key twice (no backup signature): refused.
    let error = generation_1()
        .rotate(Rotation {
            keys_json: &next,
            main_signature: Some(&main_sig),
            backup_signature: &main_sig,
        })
        .unwrap_err();
    assert!(
        matches!(
            error,
            KeyChainError::Signature {
                which: "backup",
                source: SignatureError::WrongKeyId { .. },
                ..
            }
        ),
        "{error}"
    );

    // A generation that skips one, or a signature moved from another
    // generation, is refused.
    let skip = keys_json(3, &main2, &backup2, &[]);
    let error = generation_1()
        .rotate(Rotation {
            keys_json: &skip,
            main_signature: Some(&main1.sign(&skip, comment)),
            backup_signature: &backup1.sign(&skip, comment),
        })
        .unwrap_err();
    assert_eq!(
        error,
        KeyChainError::GenerationOrder {
            expected: 2,
            found: 3
        }
    );
    let moved = "rs-rich-marketplace keys generation=3";
    let error = generation_1()
        .rotate(Rotation {
            keys_json: &next,
            main_signature: Some(&main1.sign(&next, moved)),
            backup_signature: &backup1.sign(&next, moved),
        })
        .unwrap_err();
    assert!(
        matches!(
            error,
            KeyChainError::Signature {
                source: SignatureError::TrustedComment { .. },
                ..
            }
        ),
        "{error}"
    );
}

#[test]
fn the_backup_key_alone_replaces_a_lost_main_key() {
    let (main1, backup1, main2, backup2) = (
        pair("gen1-main"),
        pair("gen1-backup"),
        pair("gen2-main"),
        pair("gen2-backup"),
    );
    let comment = "rs-rich-marketplace keys generation=2";

    // Revokes the old main key and keeps the backup: accepted, and said.
    let next = keys_json(2, &main2, &backup1, &[&main1]);
    let backup_sig = backup1.sign(&next, comment);
    let (keys, kinds) = generation_1()
        .walk([Rotation {
            keys_json: &next,
            main_signature: None,
            backup_signature: &backup_sig,
        }])
        .unwrap();
    assert_eq!(
        kinds,
        [RotationKind::MainKeyReplaced {
            revoked: main1.trusted.id()
        }]
    );
    assert!(keys.revoked().contains(&main1.trusted.id()));

    // The leaked main key's indexes are refused from now on.
    let market = Marketplace::build();
    let index = market.index(SERIAL);
    let leaked = verify_index(
        &keys,
        &index,
        &sign_index(&main1, &index, SERIAL),
        None,
        now(),
    );
    assert!(matches!(
        leaked,
        Err(IndexError::Signature(SignatureError::WrongKeyId { .. }))
    ));
    assert!(verify_index(
        &keys,
        &index,
        &sign_index(&main2, &index, SERIAL),
        None,
        now()
    )
    .is_ok());

    // The backup key alone may not skip the revocation…
    let quiet = keys_json(2, &main2, &backup1, &[]);
    let error = generation_1()
        .rotate(Rotation {
            keys_json: &quiet,
            main_signature: None,
            backup_signature: &backup1.sign(&quiet, comment),
        })
        .unwrap_err();
    assert_eq!(
        error,
        KeyChainError::MainSignatureRequired { generation: 2 }
    );

    // …nor replace itself.
    let swap = keys_json(2, &main2, &backup2, &[&main1]);
    let error = generation_1()
        .rotate(Rotation {
            keys_json: &swap,
            main_signature: None,
            backup_signature: &backup1.sign(&swap, comment),
        })
        .unwrap_err();
    assert_eq!(
        error,
        KeyChainError::BackupReplacedWithoutMain { generation: 2 }
    );

    // And the main key alone (a leaked one) cannot rotate at all.
    let error = generation_1()
        .rotate(Rotation {
            keys_json: &next,
            main_signature: Some(&main1.sign(&next, comment)),
            backup_signature: &main1.sign(&next, comment),
        })
        .unwrap_err();
    assert!(
        matches!(
            error,
            KeyChainError::Signature {
                which: "backup",
                ..
            }
        ),
        "{error}"
    );
}

#[test]
fn keys_files_are_checked() {
    let (main1, backup1) = (pair("gen1-main"), pair("gen1-backup"));
    let good = keys_json(1, &main1, &backup1, &[]);
    assert_eq!(KeySet::from_json(&good).unwrap(), generation_1());

    let text = String::from_utf8(good).unwrap();
    let wrong_id = text.replacen(&main1.trusted.id().to_string(), "0123456789ABCDEF", 1);
    assert!(matches!(
        KeySet::from_json(wrong_id.as_bytes()),
        Err(KeyChainError::BadKeysFile(_))
    ));
    let same = keys_json(1, &main1, &main1, &[]);
    assert!(KeySet::from_json(&same).is_err());
    let v2 = text.replacen("\"schema_version\": 1", "\"schema_version\": 2", 1);
    assert_eq!(
        KeySet::from_json(v2.as_bytes()),
        Err(KeyChainError::UnsupportedSchema(2))
    );
}

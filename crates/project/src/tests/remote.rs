//! Remote asset download and destination-filtering tests.

use super::*;

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn remote_asset_destination_rejects_non_public_and_encoded_ip_literals() {
    let resolver = PublicRemoteAssetResolver;
    for source in [
        "http://127.0.0.1/asset",
        "http://2130706433/asset",
        "http://0x7f000001/asset",
        "http://0177.1/asset",
        "http://[::1]/asset",
        "http://[2001:2::1]/asset",
        "http://[2200::1]/asset",
        "http://[2d00::1]/asset",
        "http://[2e00::1]/asset",
        "http://[2f00::1]/asset",
        "http://[3000::1]/asset",
        "http://[3800::1]/asset",
        "http://[3c00::1]/asset",
        "http://[3e00::1]/asset",
        "http://[3f00::1]/asset",
        "http://[3f80::1]/asset",
        "http://[3fc0::1]/asset",
        "http://[3fe0::1]/asset",
        "http://[3ff0::1]/asset",
        "http://[3ff8::1]/asset",
        "http://[3ffc::1]/asset",
        "http://[3ffe::1]/asset",
        "http://[3fff::1]/asset",
        "http://localhost/asset",
        "http://169.254.169.254/latest/meta-data/",
    ] {
        let url = reqwest::Url::parse(source).unwrap();
        assert!(
            resolver.resolve(&url).is_err(),
            "expected {source} to be rejected (parsed as {url})"
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn remote_asset_destination_rejects_mixed_dns_answers() {
    let public = SocketAddr::from(([93, 184, 216, 34], 443));
    let private = SocketAddr::from(([10, 0, 0, 7], 443));
    let public_ipv6 = "2606:4700:4700::1111".parse().unwrap();
    let reachable_special_ipv6 = "2001:1::1".parse().unwrap();

    assert!(validate_public_addresses(&[public]).is_ok());
    assert!(is_public_ip(public_ipv6));
    assert!(is_public_ip(reachable_special_ipv6));
    assert!(validate_public_addresses(&[public, private])
        .unwrap_err()
        .contains("not a public IP"));
    assert!(validate_public_addresses(&[])
        .unwrap_err()
        .contains("no addresses"));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn remote_asset_resolver_accepts_public_and_rejects_private_ipv6_literals() {
    let resolver = PublicRemoteAssetResolver;
    let public = reqwest::Url::parse("https://[2606:4700:4700::1111]/asset").unwrap();
    let globally_reachable_exception = reqwest::Url::parse("https://[2001:1::1]/asset").unwrap();
    let private = reqwest::Url::parse("https://[fd00::1]/asset").unwrap();

    assert_eq!(
        resolver.resolve(&public).unwrap(),
        vec![SocketAddr::new(
            "2606:4700:4700::1111".parse().unwrap(),
            443
        )]
    );
    assert!(resolver.resolve(&globally_reachable_exception).is_ok());
    assert!(resolver.resolve(&private).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn remote_asset_redirect_targets_are_resolved_and_rechecked() {
    let public_ip = reqwest::Url::parse("https://93.184.216.34/assets/clip.png").unwrap();
    let internal = public_ip.join("http://127.0.0.1/admin").unwrap();
    let credentials = public_ip
        .join("https://user:pass@cdn.example/asset")
        .unwrap();
    let resolver = PublicRemoteAssetResolver;

    assert!(resolver.resolve(&public_ip).is_ok());
    assert!(resolver.resolve(&internal).is_err());
    assert!(validate_remote_url(&credentials)
        .unwrap_err()
        .contains("credentials"));
    assert!(validate_remote_url(&reqwest::Url::parse("file:///etc/passwd").unwrap()).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn materializer_rechecks_redirect_destination_before_connecting() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 512];
        let _ = stream.read(&mut request);
        write!(
            stream,
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{}/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            address.port()
        )
        .unwrap();
    });
    let mut p = project();
    p.assets = vec![AssetRef {
        id: "redirected".into(),
        path: format!("http://asset.test:{}/start", address.port()),
        kind: AssetKind::Other,
        sha256: None,
    }];
    let resolver = |url: &reqwest::Url| {
        if url.host_str() == Some("asset.test") {
            Ok(vec![address])
        } else {
            PublicRemoteAssetResolver.resolve(url)
        }
    };
    let cache = tempfile::tempdir().unwrap();

    assert!(matches!(
        p.materialize_remote_assets_with_resolver(cache.path(), 1024, 1024, &resolver),
        Err(ProjectError::AssetRead { reason, .. }) if reason.contains("not a public IP")
    ));
    server.join().unwrap();
}

#[test]
fn materializes_remote_assets_and_rewrites_clip_props() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let payload = b"remote project asset".to_vec();
    let server_payload = payload.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 1024];
        let _ = stream.read(&mut request);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            server_payload.len()
        )
        .unwrap();
        stream.write_all(&server_payload).unwrap();
    });

    let cache =
        std::env::temp_dir().join(format!("rust-motion-remote-assets-{}", std::process::id()));
    let url = format!("http://asset.test:{}/poster.png", address.port());
    let digest = format!("{:x}", Sha256::digest(&payload));
    let mut p = project();
    p.assets = vec![AssetRef {
        id: "poster".into(),
        path: url.clone(),
        kind: AssetKind::Image,
        sha256: Some(digest),
    }];
    p.props = serde_json::json!({"poster": "asset://poster"});
    p.tracks = vec![Track {
        id: "track".into(),
        clips: vec![Clip {
            id: "clip".into(),
            composition: "shorts".into(),
            start: 0,
            duration: 1,
            props: serde_json::json!({"src": url}),
        }],
    }];

    let resolver = |_: &reqwest::Url| Ok(vec![address]);
    p.materialize_remote_assets_with_resolver(&cache, 1024, 1024, &resolver)
        .unwrap();
    let local = std::path::PathBuf::from(&p.assets[0].path);
    assert_eq!(std::fs::read(&local).unwrap(), payload);
    assert_eq!(p.props["poster"], local.to_string_lossy().as_ref());
    assert_eq!(
        p.tracks[0].clips[0].props["src"],
        local.to_string_lossy().as_ref()
    );
    server.join().unwrap();
    std::fs::remove_dir_all(cache).unwrap();
}

#[test]
fn materializer_rejects_unknown_length_response_over_limit() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 256];
        let _ = stream.read(&mut request);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
            .unwrap();
        stream.write_all(&[7; 2048]).unwrap();
    });
    let mut p = project();
    p.assets = vec![AssetRef {
        id: "large".into(),
        path: format!("http://asset.test:{}/large.bin", address.port()),
        kind: AssetKind::Other,
        sha256: None,
    }];
    let cache =
        std::env::temp_dir().join(format!("rust-motion-remote-limit-{}", std::process::id()));
    let resolver = |_: &reqwest::Url| Ok(vec![address]);
    assert!(matches!(
        p.materialize_remote_assets_with_resolver(&cache, 1024, 2048, &resolver),
        Err(ProjectError::AssetRead { reason, .. }) if reason.contains("exceeds")
    ));
    server.join().unwrap();
    let _ = std::fs::remove_dir_all(cache);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn materializer_enforces_total_limit_for_unknown_length_responses() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for (index, payload) in [b"abcd".to_vec(), b"xyz".to_vec()].into_iter().enumerate() {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 512];
            let _ = stream.read(&mut request);
            if index == 0 {
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                )
                .unwrap();
            } else {
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
                    .unwrap();
            }
            let _ = stream.write_all(&payload);
        }
    });

    let first = format!("http://asset.test:{}/first.bin", address.port());
    let second = format!("http://asset.test:{}/second.bin", address.port());
    let mut p = project();
    p.assets = vec![
        AssetRef {
            id: "first".into(),
            path: first,
            kind: AssetKind::Other,
            sha256: None,
        },
        AssetRef {
            id: "second".into(),
            path: second.clone(),
            kind: AssetKind::Other,
            sha256: None,
        },
    ];
    let cache = tempfile::tempdir().unwrap();
    let resolver = |_: &reqwest::Url| Ok(vec![address]);

    assert!(matches!(
        p.materialize_remote_assets_with_resolver(cache.path(), 8, 6, &resolver),
        Err(ProjectError::AssetRead { reason, .. }) if reason.contains("6 byte total limit")
    ));
    server.join().unwrap();
    assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 1);
    assert_eq!(p.assets[1].path, second);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn materializer_rejects_declared_size_over_total_limit_before_caching() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 512];
        let _ = stream.read(&mut request);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nabcd")
            .unwrap();
    });
    let mut p = project();
    p.assets = vec![AssetRef {
        id: "oversized-total".into(),
        path: format!("http://asset.test:{}/asset.bin", address.port()),
        kind: AssetKind::Other,
        sha256: None,
    }];
    let cache = tempfile::tempdir().unwrap();
    let resolver = |_: &reqwest::Url| Ok(vec![address]);

    assert!(matches!(
        p.materialize_remote_assets_with_resolver(cache.path(), 8, 3, &resolver),
        Err(ProjectError::AssetRead { reason, .. }) if reason.contains("3 byte total limit")
    ));
    server.join().unwrap();
    assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 0);
}

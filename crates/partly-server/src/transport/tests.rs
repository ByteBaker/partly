// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

#[test]
fn https_accepts_http2_authority_and_rejects_conflicting_host() {
    let transport = Transport {
        tls: true,
        hostname: Some("flags.example.com".into()),
    };
    let mut request = Request::builder()
        .uri("https://flags.example.com:8443/api/status")
        .version(axum::http::Version::HTTP_2)
        .header(header::ORIGIN, "https://flags.example.com:8443")
        .body(Body::empty())
        .unwrap();
    assert!(check_request(&request, &transport).is_ok());
    request
        .headers_mut()
        .insert(header::HOST, HeaderValue::from_static("localhost:8443"));
    assert_eq!(
        check_request(&request, &transport).unwrap_err().0,
        StatusCode::MISDIRECTED_REQUEST
    );
    request.headers_mut().remove(header::HOST);
    request.headers_mut().insert(
        header::ORIGIN,
        HeaderValue::from_static("http://flags.example.com:8443"),
    );
    request
        .headers_mut()
        .insert("x-forwarded-proto", HeaderValue::from_static("http"));
    assert_eq!(
        check_request(&request, &transport).unwrap_err().0,
        StatusCode::FORBIDDEN
    );
}

#[test]
fn hostname_configuration_is_explicit_and_has_no_scheme_or_port() {
    assert_eq!(
        parse_hostname("Flags.Example.com").unwrap(),
        "flags.example.com"
    );
    for invalid in [
        "",
        "https://flags.example.com",
        "flags.example.com:443",
        "user@host",
        "*.example.com",
        "a..b",
        "-a.com",
        "a-.com",
        "a/b",
        "a b",
        "a\\nb",
    ] {
        assert!(parse_hostname(invalid).is_err(), "{invalid}");
    }
    let request = Request::builder()
        .uri("/api/status")
        .header(header::HOST, "flags.example.com:8443")
        .body(Body::empty())
        .unwrap();
    let transport = Transport {
        tls: true,
        hostname: None,
    };
    assert_eq!(
        check_request(&request, &transport).unwrap_err().0,
        StatusCode::MISDIRECTED_REQUEST
    );
}

#[test]
fn https_origin_omits_default_port_and_lowercases_hostname() {
    let transport = Transport {
        tls: true,
        hostname: Some("flags.example.com".into()),
    };
    let request = Request::builder()
        .uri("/api/status")
        .header(header::HOST, "Flags.Example.com:443")
        .header(header::ORIGIN, "https://flags.example.com")
        .body(Body::empty())
        .unwrap();
    assert!(check_request(&request, &transport).is_ok());
}

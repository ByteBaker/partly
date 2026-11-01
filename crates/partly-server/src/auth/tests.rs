// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

#[test]
fn throttle_is_bounded_and_recovers_after_window() {
    let auth = Auth::new();
    let now = Instant::now();
    let peer = "127.0.0.1".parse().unwrap();
    for _ in 0..ATTEMPTS {
        assert!(auth.check_attempt_at(peer, now).is_ok());
    }
    assert_eq!(
        auth.check_attempt_at(peer, now).unwrap_err().0,
        axum::http::StatusCode::TOO_MANY_REQUESTS
    );
    for address in 1..MAX_PEERS {
        assert!(
            auth.check_attempt_at(std::net::Ipv4Addr::from(address as u32).into(), now)
                .is_ok()
        );
    }
    assert!(
        auth.check_attempt_at("192.0.2.1".parse().unwrap(), now)
            .is_err()
    );
    assert_eq!(auth.attempts.lock().unwrap().len(), MAX_PEERS);
    assert!(auth.check_attempt_at(peer, now + WINDOW).is_ok());
    assert_eq!(auth.attempts.lock().unwrap().len(), 1);
}

use crate::ApiError;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

const WINDOW: Duration = Duration::from_secs(60);
const ATTEMPTS: u8 = 5;
const MAX_PEERS: usize = 1024;

pub struct Auth {
    attempts: Mutex<HashMap<IpAddr, (Instant, u8)>>,
    workers: Arc<Semaphore>,
}

impl Auth {
    pub fn new() -> Self {
        Self {
            attempts: Mutex::new(HashMap::new()),
            workers: Arc::new(Semaphore::new(2)),
        }
    }

    pub fn check_attempt(&self, peer: IpAddr) -> Result<(), ApiError> {
        self.check_attempt_at(peer.to_canonical(), Instant::now())
    }

    fn check_attempt_at(&self, peer: IpAddr, now: Instant) -> Result<(), ApiError> {
        let mut attempts = self
            .attempts
            .lock()
            .map_err(|_| ApiError::internal("Authentication lock failed"))?;
        attempts.retain(|_, (start, _)| now.duration_since(*start) < WINDOW);
        if !attempts.contains_key(&peer) && attempts.len() >= MAX_PEERS {
            return Err(ApiError::throttled());
        }
        let (_, count) = attempts.entry(peer).or_insert((now, 0));
        if *count >= ATTEMPTS {
            return Err(ApiError::throttled());
        }
        *count += 1;
        Ok(())
    }

    pub async fn run<T, F>(&self, operation: F) -> Result<T, ApiError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, ApiError> + Send + 'static,
    {
        let permit = self
            .workers
            .clone()
            .try_acquire_owned()
            .map_err(|_| ApiError::throttled())?;
        tokio::task::spawn_blocking(move || {
            // Keep the permit until CPU work ends, even if the HTTP request is cancelled.
            let _permit = permit;
            operation()
        })
        .await
        .map_err(|_| ApiError::internal("Authentication task failed"))?
    }
}

pub fn validate_password(password: &str) -> Result<(), ApiError> {
    if !(12..=1024).contains(&password.len()) {
        return Err(ApiError::bad("Use a password of 12–1024 bytes"));
    }
    Ok(())
}

pub fn hash_password(password: &str) -> Result<String, ApiError> {
    validate_password(password)?;
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|_| ApiError::internal("Cannot hash password"))
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool, ApiError> {
    let parsed =
        PasswordHash::new(hash).map_err(|_| ApiError::internal("Invalid stored password hash"))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

#[cfg(test)]
mod tests {
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
            assert!(auth
                .check_attempt_at(std::net::Ipv4Addr::from(address as u32).into(), now)
                .is_ok());
        }
        assert!(auth
            .check_attempt_at("192.0.2.1".parse().unwrap(), now)
            .is_err());
        assert_eq!(auth.attempts.lock().unwrap().len(), MAX_PEERS);
        assert!(auth.check_attempt_at(peer, now + WINDOW).is_ok());
        assert_eq!(auth.attempts.lock().unwrap().len(), 1);
    }
}

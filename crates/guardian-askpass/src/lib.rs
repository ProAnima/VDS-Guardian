//! One-shot `SSH_ASKPASS` plumbing for password logins.
//!
//! OpenSSH can only receive a password from a terminal or from an `SSH_ASKPASS`
//! program. Neither argv, the environment, a shell string nor a file may carry
//! the secret, so the password travels over a single-use loopback connection:
//!
//! 1. the application starts a [`Broker`] that holds the password in memory and
//!    listens on `127.0.0.1` on an ephemeral port;
//! 2. it spawns `ssh` with `SSH_ASKPASS` pointing at the `guardian-askpass`
//!    binary and only the port and a 256-bit one-time token in the environment
//!    (neither is a secret on its own);
//! 3. the helper checks that the prompt is the login password prompt, connects,
//!    presents the token, receives the password once and prints it to the pipe
//!    OpenSSH reads from.
//!
//! The broker answers a single valid request, rejects everything else, and stops
//! after a deadline or when dropped; its copy of the password is zeroized.

use rand_core::{OsRng, RngCore};
use std::{
    io::{self, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub const PORT_VARIABLE: &str = "GUARDIAN_ASKPASS_PORT";
pub const TOKEN_VARIABLE: &str = "GUARDIAN_ASKPASS_TOKEN";

/// Longest accepted password (bytes); the same bound is enforced when it is stored.
pub const MAX_PASSWORD_BYTES: usize = 256;
const TOKEN_HEX_LENGTH: usize = 64;
const IO_TIMEOUT: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_PROMPT_BYTES: usize = 512;

const STATUS_OK: u8 = 0;
const STATUS_DENIED: u8 = 1;

#[derive(Debug, PartialEq, Eq)]
pub enum AskpassError {
    /// Not the login password prompt; the secret is never offered for any other prompt.
    UnexpectedPrompt,
    InvalidConfiguration,
    Refused,
    Io,
}

impl std::fmt::Display for AskpassError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::UnexpectedPrompt => "not the login password prompt",
            Self::InvalidConfiguration => "invalid askpass configuration",
            Self::Refused => "password request refused",
            Self::Io => "local connection failed",
        })
    }
}

impl std::error::Error for AskpassError {}

/// Only the plain login prompt qualifies: `<user>@<host>'s password:` or `Password:`.
/// Anything else (a key passphrase, a host-key question, `(current) UNIX password:`,
/// `New password:`) is refused so the stored password is never typed into the wrong place.
#[must_use]
pub fn is_login_password_prompt(prompt: &str) -> bool {
    if prompt.len() > MAX_PROMPT_BYTES {
        return false;
    }
    let prompt = prompt.trim();
    if prompt.chars().any(char::is_control) {
        return false;
    }
    let lower = prompt.to_ascii_lowercase();
    if lower == "password:" {
        return true;
    }
    lower
        .strip_suffix("'s password:")
        .is_some_and(|target| target.contains('@') && !target.contains(char::is_whitespace))
}

#[must_use]
pub fn is_valid_token(token: &str) -> bool {
    token.len() == TOKEN_HEX_LENGTH
        && token
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
            == 0
}

/// Serves one password to one authenticated local request.
pub struct Broker {
    port: u16,
    token: String,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Broker {
    /// Starts listening immediately; `lifetime` bounds how long the password stays in memory.
    pub fn start(password: &[u8], lifetime: Duration) -> io::Result<Self> {
        if password.is_empty() || password.len() > MAX_PASSWORD_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "password length",
            ));
        }
        let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let token = random_token();
        let stop = Arc::new(AtomicBool::new(false));
        let secret = Zeroizing::new(password.to_vec());
        let expected = token.clone();
        let stopping = Arc::clone(&stop);
        let join = thread::spawn(move || serve(&listener, &expected, &secret, lifetime, &stopping));
        Ok(Self {
            port,
            token,
            stop,
            join: Some(join),
        })
    }

    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Hex one-time token; not a secret by itself, it only authorises one request for the password.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn random_token() -> String {
    let mut bytes = [0_u8; TOKEN_HEX_LENGTH / 2];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn serve(
    listener: &TcpListener,
    token: &str,
    password: &[u8],
    lifetime: Duration,
    stop: &AtomicBool,
) {
    let deadline = Instant::now() + lifetime;
    while !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
        match listener.accept() {
            Ok((stream, peer)) => {
                if peer.ip().is_loopback() && answer(stream, token, password) {
                    return;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL_INTERVAL),
            Err(_) => return,
        }
    }
}

/// `true` only after the password was handed to a connection that proved the token.
fn answer(mut stream: TcpStream, token: &str, password: &[u8]) -> bool {
    if stream.set_nonblocking(false).is_err()
        || stream.set_read_timeout(Some(IO_TIMEOUT)).is_err()
        || stream.set_write_timeout(Some(IO_TIMEOUT)).is_err()
    {
        return false;
    }
    let mut presented = [0_u8; TOKEN_HEX_LENGTH];
    if stream.read_exact(&mut presented).is_err()
        || !constant_time_equal(&presented, token.as_bytes())
    {
        let _ = stream.write_all(&[STATUS_DENIED]);
        return false;
    }
    let Ok(length) = u16::try_from(password.len()) else {
        return false;
    };
    let mut response = Zeroizing::new(Vec::with_capacity(3 + password.len()));
    response.push(STATUS_OK);
    response.extend_from_slice(&length.to_be_bytes());
    response.extend_from_slice(password);
    stream
        .write_all(&response)
        .and_then(|()| stream.flush())
        .is_ok()
}

/// Helper side: ask the broker for the password after checking the prompt.
pub fn request_password(
    prompt: &str,
    port: &str,
    token: &str,
) -> Result<Zeroizing<Vec<u8>>, AskpassError> {
    if !is_login_password_prompt(prompt) {
        return Err(AskpassError::UnexpectedPrompt);
    }
    let port: u16 = port
        .parse()
        .map_err(|_| AskpassError::InvalidConfiguration)?;
    if port == 0 || !is_valid_token(token) {
        return Err(AskpassError::InvalidConfiguration);
    }
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let mut stream =
        TcpStream::connect_timeout(&address, IO_TIMEOUT).map_err(|_| AskpassError::Io)?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
        .map_err(|_| AskpassError::Io)?;
    stream
        .write_all(token.as_bytes())
        .map_err(|_| AskpassError::Io)?;
    let mut status = [0_u8; 1];
    stream
        .read_exact(&mut status)
        .map_err(|_| AskpassError::Refused)?;
    if status[0] != STATUS_OK {
        return Err(AskpassError::Refused);
    }
    let mut length = [0_u8; 2];
    stream
        .read_exact(&mut length)
        .map_err(|_| AskpassError::Io)?;
    let length = usize::from(u16::from_be_bytes(length));
    if length == 0 || length > MAX_PASSWORD_BYTES {
        return Err(AskpassError::Refused);
    }
    let mut password = Zeroizing::new(vec![0_u8; length]);
    stream
        .read_exact(&mut password)
        .map_err(|_| AskpassError::Io)?;
    Ok(password)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetch(broker: &Broker) -> Result<Zeroizing<Vec<u8>>, AskpassError> {
        request_password(
            "backup@vds.example's password: ",
            &broker.port().to_string(),
            broker.token(),
        )
    }

    #[test]
    fn only_the_plain_login_prompt_is_answered() {
        for accepted in [
            "backup@vds.example's password: ",
            "root@10.0.0.1's password:",
            "Password: ",
            "password:",
        ] {
            assert!(is_login_password_prompt(accepted), "{accepted:?}");
        }
        for refused in [
            "Enter passphrase for key '/home/u/.ssh/id_ed25519': ",
            "Are you sure you want to continue connecting (yes/no/[fingerprint])? ",
            "(current) UNIX password: ",
            "New password: ",
            "Retype new password: ",
            "Verification code: ",
            "",
            "password",
        ] {
            assert!(!is_login_password_prompt(refused), "{refused:?}");
        }
    }

    #[test]
    fn the_broker_hands_the_password_to_a_request_with_the_token()
    -> Result<(), Box<dyn std::error::Error>> {
        let broker = Broker::start("pässw0rd 密码!".as_bytes(), Duration::from_secs(10))?;
        assert_eq!(fetch(&broker)?.as_slice(), "pässw0rd 密码!".as_bytes());
        Ok(())
    }

    #[test]
    fn a_wrong_token_is_denied_and_does_not_consume_the_password()
    -> Result<(), Box<dyn std::error::Error>> {
        let broker = Broker::start(b"secret", Duration::from_secs(10))?;
        let wrong = "0".repeat(TOKEN_HEX_LENGTH);
        let denied = request_password("Password:", &broker.port().to_string(), &wrong);
        assert_eq!(denied, Err(AskpassError::Refused));
        assert_eq!(fetch(&broker)?.as_slice(), b"secret");
        Ok(())
    }

    #[test]
    fn the_password_is_served_exactly_once() -> Result<(), Box<dyn std::error::Error>> {
        let broker = Broker::start(b"secret", Duration::from_secs(10))?;
        assert!(fetch(&broker).is_ok());
        thread::sleep(Duration::from_millis(100));
        assert!(fetch(&broker).is_err());
        Ok(())
    }

    #[test]
    fn dropping_the_broker_closes_the_port() -> Result<(), Box<dyn std::error::Error>> {
        let broker = Broker::start(b"secret", Duration::from_secs(10))?;
        let (port, token) = (broker.port().to_string(), broker.token().to_owned());
        drop(broker);
        assert!(request_password("Password:", &port, &token).is_err());
        Ok(())
    }

    #[test]
    fn the_broker_stops_listening_after_its_lifetime() -> Result<(), Box<dyn std::error::Error>> {
        let broker = Broker::start(b"secret", Duration::from_millis(50))?;
        thread::sleep(Duration::from_millis(300));
        assert!(fetch(&broker).is_err());
        Ok(())
    }

    #[test]
    fn a_prompt_mismatch_never_contacts_the_broker() -> Result<(), Box<dyn std::error::Error>> {
        let broker = Broker::start(b"secret", Duration::from_secs(10))?;
        let result = request_password(
            "Enter passphrase for key 'k': ",
            &broker.port().to_string(),
            broker.token(),
        );
        assert_eq!(result, Err(AskpassError::UnexpectedPrompt));
        assert_eq!(fetch(&broker)?.as_slice(), b"secret");
        Ok(())
    }

    #[test]
    fn tokens_are_unique_hex_and_validated() -> Result<(), Box<dyn std::error::Error>> {
        let (first, second) = (
            Broker::start(b"a", Duration::from_secs(1))?,
            Broker::start(b"a", Duration::from_secs(1))?,
        );
        assert_ne!(first.token(), second.token());
        assert!(is_valid_token(first.token()));
        for bad in [
            "",
            "short",
            &"A".repeat(64),
            &"g".repeat(64),
            &"0".repeat(63),
        ] {
            assert!(!is_valid_token(bad), "{bad:?}");
        }
        Ok(())
    }

    #[test]
    fn empty_or_oversized_passwords_are_rejected_up_front() {
        assert!(Broker::start(b"", Duration::from_secs(1)).is_err());
        assert!(Broker::start(&[b'x'; MAX_PASSWORD_BYTES + 1], Duration::from_secs(1)).is_err());
    }
}

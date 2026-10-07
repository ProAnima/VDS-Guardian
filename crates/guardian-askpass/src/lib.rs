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
        atomic::{AtomicBool, AtomicUsize, Ordering},
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
/// Helper-side timeout for connecting and for each read and write.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);
/// Total time one connection may take on the broker side, however slowly it trickles bytes.
const CONNECTION_BUDGET: Duration = Duration::from_millis(1500);
/// Connections handled at once; further ones are closed immediately.
const MAX_IN_FLIGHT: usize = 8;
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

/// Only the plain login prompt qualifies: `<user>@<host>'s password:`. Anything else (a key
/// passphrase, a host-key question, a keyboard-interactive `Password:` (disabled for our logins),
/// `(current) UNIX password:`, `New password:`) is refused so the stored password is never typed
/// into the wrong place.
#[must_use]
pub fn is_login_password_prompt(prompt: &str) -> bool {
    if prompt.len() > MAX_PROMPT_BYTES {
        return false;
    }
    let prompt = prompt.trim();
    if prompt.chars().any(char::is_control) {
        return false;
    }
    prompt
        .to_ascii_lowercase()
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

/// State shared by the accept loop and the short-lived connection workers.
struct Shared {
    token: String,
    password: Zeroizing<Vec<u8>>,
    expires: Instant,
    served: AtomicBool,
    stopped: AtomicBool,
    in_flight: AtomicUsize,
}

impl Shared {
    fn open(&self) -> bool {
        !self.served.load(Ordering::SeqCst)
            && !self.stopped.load(Ordering::SeqCst)
            && Instant::now() < self.expires
    }
}

/// Serves one password to one authenticated local request.
pub struct Broker {
    port: u16,
    token: String,
    shared: Arc<Shared>,
    join: Option<JoinHandle<()>>,
}

impl Broker {
    /// Starts listening immediately; `lifetime` bounds how long the password can be handed out.
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
        let shared = Arc::new(Shared {
            token: token.clone(),
            password: Zeroizing::new(password.to_vec()),
            expires: Instant::now() + lifetime,
            served: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            in_flight: AtomicUsize::new(0),
        });
        let accepting = Arc::clone(&shared);
        let join = thread::Builder::new()
            .name("guardian-askpass-broker".to_owned())
            .spawn(move || serve(&listener, &accepting))?;
        Ok(Self {
            port,
            token,
            shared,
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
    /// Stops accepting at once. The accept loop polls, so joining it is quick; connection workers
    /// still running end within their budget and refuse to serve once stopped.
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
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

/// Accepts until the password is served, the broker is dropped or it expires. A slow or hostile
/// local client only occupies its own worker for at most `CONNECTION_BUDGET`, and transient
/// accept errors never end the broker early.
fn serve(listener: &TcpListener, shared: &Arc<Shared>) {
    while shared.open() {
        match listener.accept() {
            Ok((stream, peer)) if peer.ip().is_loopback() => dispatch(stream, shared),
            Ok(_) => {}
            Err(_) => thread::sleep(POLL_INTERVAL),
        }
    }
}

fn dispatch(stream: TcpStream, shared: &Arc<Shared>) {
    if shared.in_flight.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
        shared.in_flight.fetch_sub(1, Ordering::SeqCst);
        return;
    }
    let worker = Arc::clone(shared);
    let spawned = thread::Builder::new().spawn(move || {
        answer(stream, &worker);
        worker.in_flight.fetch_sub(1, Ordering::SeqCst);
    });
    if spawned.is_err() {
        shared.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Reads the token within one overall budget, then hands the password over at most once.
fn answer(mut stream: TcpStream, shared: &Shared) {
    let deadline = Instant::now() + CONNECTION_BUDGET;
    if stream.set_nonblocking(false).is_err() {
        return;
    }
    let mut presented = [0_u8; TOKEN_HEX_LENGTH];
    let mut filled = 0;
    while filled < TOKEN_HEX_LENGTH {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || stream.set_read_timeout(Some(remaining)).is_err() {
            return;
        }
        match stream.read(&mut presented[filled..]) {
            Ok(0) | Err(_) => return,
            Ok(read) => filled += read,
        }
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() || stream.set_write_timeout(Some(remaining)).is_err() {
        return;
    }
    let granted = constant_time_equal(&presented, shared.token.as_bytes())
        && shared.open()
        && shared
            .served
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();
    if !granted {
        let _ = stream.write_all(&[STATUS_DENIED]);
        return;
    }
    let Ok(length) = u16::try_from(shared.password.len()) else {
        return;
    };
    let mut response = Zeroizing::new(Vec::with_capacity(3 + shared.password.len()));
    response.push(STATUS_OK);
    response.extend_from_slice(&length.to_be_bytes());
    response.extend_from_slice(&shared.password);
    let _ = stream.write_all(&response).and_then(|()| stream.flush());
}

/// Entry point for any binary that can act as the `SSH_ASKPASS` program. Call it first thing in
/// `main`. It answers only when started exactly the way OpenSSH starts an askpass program: one
/// prompt argument, `SSH_ASKPASS_REQUIRE` set, and the broker port and token in the environment.
/// Then it returns the exit code the process must end with; otherwise `None` (stray variables in
/// a user's environment are ignored and the program starts normally).
#[must_use]
pub fn run_if_requested() -> Option<i32> {
    let (port, token) = (
        std::env::var(PORT_VARIABLE).ok()?,
        std::env::var(TOKEN_VARIABLE).ok()?,
    );
    std::env::var_os("SSH_ASKPASS_REQUIRE")?;
    let arguments: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let [_, prompt] = arguments.as_slice() else {
        return None;
    };
    let prompt = prompt.to_str().unwrap_or_default();
    Some(answer_openssh(
        prompt,
        &port,
        &token,
        &mut io::stdout().lock(),
    ))
}

fn answer_openssh(prompt: &str, port: &str, token: &str, output: &mut impl Write) -> i32 {
    let Ok(password) = request_password(prompt, port, token) else {
        eprintln!("guardian-askpass: password request refused");
        return 1;
    };
    // Sized up front so appending the newline never reallocates and leaves an unwiped copy.
    let mut line = Zeroizing::new(Vec::with_capacity(password.len() + 1));
    line.extend_from_slice(&password);
    line.push(b'\n');
    if output
        .write_all(&line)
        .and_then(|()| output.flush())
        .is_ok()
    {
        0
    } else {
        1
    }
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
        TcpStream::connect_timeout(&address, CLIENT_TIMEOUT).map_err(|_| AskpassError::Io)?;
    stream
        .set_read_timeout(Some(CLIENT_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(CLIENT_TIMEOUT)))
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
            "Password: ",
            "password:",
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
        let denied = request_password("u@h's password:", &broker.port().to_string(), &wrong);
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
        assert!(request_password("u@h's password:", &port, &token).is_err());
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
    fn a_slow_local_client_cannot_block_the_legitimate_request()
    -> Result<(), Box<dyn std::error::Error>> {
        let broker = Broker::start(b"secret", Duration::from_secs(10))?;
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), broker.port());
        let mut stalls = Vec::new();
        for _ in 0..3 {
            let mut stall = TcpStream::connect(address)?;
            stall.write_all(b"0")?;
            stalls.push(stall);
        }
        let started = Instant::now();
        assert_eq!(fetch(&broker)?.as_slice(), b"secret");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "served while other clients stalled"
        );
        drop(stalls);
        Ok(())
    }

    #[test]
    fn dropping_the_broker_returns_promptly_even_with_a_stalled_client()
    -> Result<(), Box<dyn std::error::Error>> {
        let broker = Broker::start(b"secret", Duration::from_secs(10))?;
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), broker.port());
        let _stall = TcpStream::connect(address)?;
        thread::sleep(Duration::from_millis(50));
        let started = Instant::now();
        drop(broker);
        assert!(started.elapsed() < Duration::from_millis(500));
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

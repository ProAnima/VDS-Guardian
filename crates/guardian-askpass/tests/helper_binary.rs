//! Runs the real `guardian-askpass` binary the way OpenSSH does: prompt as argv[1],
//! port and token in the environment, password read from stdout.

use guardian_askpass::{Broker, PORT_VARIABLE, TOKEN_VARIABLE};
use std::{
    process::{Command, Output},
    time::Duration,
};

const PROMPT: &str = "backup@vds.example's password: ";

fn run(prompt: &str, port: Option<&str>, token: Option<&str>) -> Result<Output, std::io::Error> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_guardian-askpass"));
    // OpenSSH always passes SSH_ASKPASS_REQUIRE to the helper it starts; without it the
    // program is not acting as a helper at all.
    command
        .arg(prompt)
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env_remove(PORT_VARIABLE)
        .env_remove(TOKEN_VARIABLE);
    if let Some(port) = port {
        command.env(PORT_VARIABLE, port);
    }
    if let Some(token) = token {
        command.env(TOKEN_VARIABLE, token);
    }
    command.output()
}

#[test]
fn prints_exactly_the_password_and_a_newline_for_the_login_prompt()
-> Result<(), Box<dyn std::error::Error>> {
    let broker = Broker::start("S3cret-Pass!".as_bytes(), Duration::from_secs(10))?;
    let output = run(
        PROMPT,
        Some(&broker.port().to_string()),
        Some(broker.token()),
    )?;
    assert!(output.status.success());
    assert_eq!(output.stdout, b"S3cret-Pass!\n");
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn refuses_other_prompts_with_empty_output_and_leaves_the_password_unserved()
-> Result<(), Box<dyn std::error::Error>> {
    let broker = Broker::start(b"S3cret-Pass!", Duration::from_secs(10))?;
    let (port, token) = (broker.port().to_string(), broker.token().to_owned());
    for prompt in [
        "Enter passphrase for key '/k': ",
        "New password: ",
        "Are you sure (yes/no)? ",
    ] {
        let output = run(prompt, Some(&port), Some(&token))?;
        assert!(!output.status.success(), "{prompt}");
        assert!(output.stdout.is_empty(), "{prompt}");
    }
    let output = run(PROMPT, Some(&port), Some(&token))?;
    assert_eq!(output.stdout, b"S3cret-Pass!\n");
    Ok(())
}

#[test]
fn fails_silently_without_configuration_or_with_a_malformed_token()
-> Result<(), Box<dyn std::error::Error>> {
    let broker = Broker::start(b"S3cret-Pass!", Duration::from_secs(10))?;
    let port = broker.port().to_string();
    for (port_value, token_value) in [
        (None, None),
        (Some(port.as_str()), None),
        (None, Some(broker.token())),
        (Some(port.as_str()), Some("not-a-token")),
    ] {
        let output = run(PROMPT, port_value, token_value)?;
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    Ok(())
}

#[test]
fn stray_variables_without_the_askpass_call_shape_are_ignored()
-> Result<(), Box<dyn std::error::Error>> {
    let broker = Broker::start(b"S3cret-Pass!", Duration::from_secs(10))?;
    let output = Command::new(env!("CARGO_BIN_EXE_guardian-askpass"))
        .arg(PROMPT)
        .env_remove("SSH_ASKPASS_REQUIRE")
        .env(PORT_VARIABLE, broker.port().to_string())
        .env(TOKEN_VARIABLE, broker.token())
        .output()?;
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    Ok(())
}

#[test]
fn the_password_never_appears_on_stderr_even_when_refused() -> Result<(), Box<dyn std::error::Error>>
{
    let broker = Broker::start(b"S3cret-Pass!", Duration::from_secs(10))?;
    let wrong = "0".repeat(64);
    let output = run(PROMPT, Some(&broker.port().to_string()), Some(&wrong))?;
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("S3cret"));
    Ok(())
}

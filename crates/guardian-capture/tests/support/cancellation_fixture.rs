use super::{Container, FIXTURE_USER, run};
use std::{
    error::Error,
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

impl Container {
    /// Restricts this fixture key to a test-only forced command that throttles
    /// an outgoing filesystem capture after it emitted its first byte. The
    /// marker is written only for the real `tar --create` capture command.
    pub fn install_throttled_capture_key(
        &self,
        public_key_path: &Path,
        workdir: &Path,
    ) -> Result<&'static str, Box<dyn Error>> {
        const MARKER: &str = "/tmp/guardian-capture-stream-started";
        let wrapper = workdir.join("guardian-capture-throttle");
        fs::write(
            &wrapper,
            "#!/bin/sh\ncase \"$SSH_ORIGINAL_COMMAND\" in\n  *\"tar --create\"*)\n    sh -c \"$SSH_ORIGINAL_COMMAND\" | {\n      dd bs=1 count=1 of=/tmp/guardian-capture-first-byte 2>/dev/null || exit 1\n      touch /tmp/guardian-capture-stream-started\n      { cat /tmp/guardian-capture-first-byte; pv -q -L 1048576; }\n    }\n    status=$?\n    rm -f /tmp/guardian-capture-first-byte\n    exit \"$status\"\n    ;;\n  *) exec sh -c \"$SSH_ORIGINAL_COMMAND\" ;;\nesac\n",
        )?;
        let authorized_key = workdir.join("guardian-capture-authorized-keys");
        let key = fs::read_to_string(public_key_path)?;
        fs::write(
            &authorized_key,
            format!("command=\"/usr/local/bin/guardian-capture-throttle\" {key}"),
        )?;
        run(
            "docker",
            &[
                "cp",
                &wrapper.to_string_lossy(),
                &format!("{}:/usr/local/bin/guardian-capture-throttle", self.id),
            ],
        )?;
        run(
            "docker",
            &[
                "exec",
                &self.id,
                "chmod",
                "755",
                "/usr/local/bin/guardian-capture-throttle",
            ],
        )?;
        run(
            "docker",
            &[
                "cp",
                &authorized_key.to_string_lossy(),
                &format!("{}:/home/{FIXTURE_USER}/.ssh/authorized_keys", self.id),
            ],
        )?;
        run(
            "docker",
            &[
                "exec",
                &self.id,
                "chown",
                &format!("{FIXTURE_USER}:{FIXTURE_USER}"),
                &format!("/home/{FIXTURE_USER}/.ssh/authorized_keys"),
            ],
        )?;
        Ok(MARKER)
    }

    /// Restricts this fixture key to a test-only forced command that throttles
    /// an incoming filesystem deploy after receiving its first byte. The
    /// marker is written only for the real `tar --extract` deploy command,
    /// never for readiness or target-absence probes.
    pub fn install_throttled_deploy_key(
        &self,
        public_key_path: &Path,
        workdir: &Path,
    ) -> Result<&'static str, Box<dyn Error>> {
        const MARKER: &str = "/tmp/guardian-deploy-stream-started";
        let wrapper = workdir.join("guardian-deploy-throttle");
        fs::write(
            &wrapper,
            "#!/bin/sh\ncase \"$SSH_ORIGINAL_COMMAND\" in\n  *\"tar --extract\"*)\n    dd bs=1 count=1 of=/tmp/guardian-deploy-first-byte 2>/dev/null || exit 1\n    touch /tmp/guardian-deploy-stream-started\n    { cat /tmp/guardian-deploy-first-byte; pv -q -L 1048576; } | sh -c \"$SSH_ORIGINAL_COMMAND\"\n    status=$?\n    rm -f /tmp/guardian-deploy-first-byte\n    exit \"$status\"\n    ;;\n  *) exec sh -c \"$SSH_ORIGINAL_COMMAND\" ;;\nesac\n",
        )?;
        let authorized_key = workdir.join("guardian-deploy-authorized-keys");
        let key = fs::read_to_string(public_key_path)?;
        fs::write(
            &authorized_key,
            format!("command=\"/usr/local/bin/guardian-deploy-throttle\" {key}"),
        )?;
        run(
            "docker",
            &[
                "cp",
                &wrapper.to_string_lossy(),
                &format!("{}:/usr/local/bin/guardian-deploy-throttle", self.id),
            ],
        )?;
        run(
            "docker",
            &[
                "exec",
                &self.id,
                "chmod",
                "755",
                "/usr/local/bin/guardian-deploy-throttle",
            ],
        )?;
        run(
            "docker",
            &[
                "cp",
                &authorized_key.to_string_lossy(),
                &format!("{}:/home/{FIXTURE_USER}/.ssh/authorized_keys", self.id),
            ],
        )?;
        run(
            "docker",
            &[
                "exec",
                &self.id,
                "chown",
                &format!("{FIXTURE_USER}:{FIXTURE_USER}"),
                &format!("/home/{FIXTURE_USER}/.ssh/authorized_keys"),
            ],
        )?;
        Ok(MARKER)
    }

    pub fn add_incompressible_fixture_file(&self) -> Result<(), Box<dyn Error>> {
        run(
            "docker",
            &[
                "exec",
                &self.id,
                "sh",
                "-c",
                "dd if=/dev/urandom of=/srv/app/guardian-cancel.bin bs=1M count=4 && chown backup:backup /srv/app/guardian-cancel.bin",
            ],
        )?;
        Ok(())
    }

    pub fn wait_for_remote_file(
        &self,
        path: &str,
        deadline: Duration,
    ) -> Result<(), Box<dyn Error>> {
        let start = Instant::now();
        loop {
            if Command::new("docker")
                .args(["exec", &self.id, "test", "-f", path])
                .status()?
                .success()
            {
                return Ok(());
            }
            if start.elapsed() >= deadline {
                return Err(format!("timed out waiting for remote marker {path}").into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn wait_for_remote_absence(
        &self,
        paths: &[&str],
        deadline: Duration,
    ) -> Result<(), Box<dyn Error>> {
        let start = Instant::now();
        loop {
            let absent = paths.iter().all(|path| {
                Command::new("docker")
                    .args(["exec", &self.id, "test", "!", "-e", path])
                    .status()
                    .is_ok_and(|status| status.success())
            });
            if absent {
                return Ok(());
            }
            if start.elapsed() >= deadline {
                return Err("timed out waiting for cancelled remote deploy cleanup".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

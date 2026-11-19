use std::path::Path;

use sha1::{Digest, Sha1};
use tempfile::NamedTempFile;
use tokio::fs::{self, File};
use tokio::io::{self, AsyncWriteExt};

use super::{SshConnector, SshConnectorError};
use crate::api::{CommandBit, StringCommand};
use crate::connectors::{CommandResult, FileTransferOptions, RunOptions};

const UPLOAD_ATTEMPTS: usize = 3;

impl SshConnector {
    pub(super) async fn put(
        &self,
        source: &Path,
        destination: &str,
        transfer: FileTransferOptions<'_>,
    ) -> Result<(), SshConnectorError> {
        let options = transfer.run;
        if !requires_privilege_handoff(options) {
            return self.upload(source, destination).await;
        }

        let temporary = temporary_filename(destination, transfer.remote_temporary_filename, options);
        self.upload(source, &temporary).await?;
        let no_privilege = without_privilege(options);

        if let Some(user) = handoff_user(options) {
            let acl = acl_command(user, &temporary, false);
            let result = self.run(acl, &no_privilege).await?;
            if !result.success && is_nfs_v4_acl_mismatch(&result.output.stderr()) {
                let nfs_acl = acl_command(user, &temporary, true);
                require_success(&self.run(nfs_acl, &no_privilege).await?, "ACL handoff")?;
            } else {
                require_success(&result, "ACL handoff")?;
            }
        }

        require_success(&self.run(copy_command(&temporary, destination), options).await?, "privileged upload copy")?;
        require_success(&self.run(remove_command(&temporary), &no_privilege).await?, "upload temporary-file cleanup")
    }

    pub(super) async fn get(
        &self,
        source: &str,
        destination: &Path,
        transfer: FileTransferOptions<'_>,
    ) -> Result<(), SshConnectorError> {
        let options = transfer.run;
        if !requires_privilege_handoff(options) {
            return self.download(source, destination).await;
        }

        let temporary = temporary_filename(source, transfer.remote_temporary_filename, options);
        require_success(
            &self.run(download_stage_command(source, &temporary), options).await?,
            "privileged download copy",
        )?;

        let transfer = self.download(&temporary, destination).await;
        let cleanup = self.run(remove_command(&temporary), options).await;
        match transfer {
            Err(error) => Err(error),
            Ok(()) => require_success(&cleanup?, "download temporary-file cleanup"),
        }
    }

    async fn upload(&self, source: &Path, destination: &str) -> Result<(), SshConnectorError> {
        let mut last_error = None;
        for _ in 0..UPLOAD_ATTEMPTS {
            match self.upload_once(source, destination).await {
                Ok(()) => return Ok(()),
                Err(error) => last_error = Some(error),
            }
        }
        match last_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    async fn upload_once(&self, source: &Path, destination: &str) -> Result<(), SshConnectorError> {
        let sftp = self.sftp().await?;
        let mut source = File::open(source).await?;
        let mut destination = sftp.create(destination.to_owned()).await?;
        io::copy(&mut source, &mut destination).await?;
        destination.shutdown().await?;
        Ok(())
    }

    async fn download(&self, source: &str, destination: &Path) -> Result<(), SshConnectorError> {
        let sftp = self.sftp().await?;
        let mut source = sftp.open(source.to_owned()).await?;
        let temporary = NamedTempFile::new()?;
        let mut staged = File::from_std(temporary.reopen()?);
        io::copy(&mut source, &mut staged).await?;
        source.close().await?;
        staged.flush().await?;
        drop(staged);
        fs::copy(temporary.path(), destination).await?;
        Ok(())
    }
}

fn requires_privilege_handoff(options: &RunOptions) -> bool {
    let unix = &options.unix;
    unix.sudo || unix.doas || unix.dzdo || unix.su_user.is_some()
}

fn handoff_user(options: &RunOptions) -> Option<&str> {
    let unix = &options.unix;
    unix.su_user.as_deref().or(unix.sudo_user.as_deref()).or(unix.doas_user.as_deref()).or(unix.dzdo_user.as_deref())
}

fn without_privilege(options: &RunOptions) -> RunOptions {
    let mut options = options.clone();
    let unix = &mut options.unix;
    unix.working_directory = None;
    unix.su_user = None;
    unix.su_password = None;
    unix.su_askpass_path = None;
    unix.sudo = false;
    unix.sudo_password = None;
    unix.sudo_askpass_path = None;
    unix.doas = false;
    unix.dzdo = false;
    options
}

fn temporary_filename(hash_key: &str, explicit: Option<&str>, options: &RunOptions) -> String {
    if let Some(explicit) = explicit {
        return explicit.to_owned();
    }
    let hash = Sha1::digest(hash_key.as_bytes());
    let mut hash_hex = String::with_capacity(hash.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in hash {
        hash_hex.push(char::from(HEX[usize::from(byte >> 4)]));
        hash_hex.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    format!("{}/pyinfra-{hash_hex}", options.remote_temp_directory.trim_end_matches('/'))
}

fn acl_command(user: &str, path: &str, nfs_v4: bool) -> StringCommand {
    let permission = if nfs_v4 { format!("u:{user}:r::allow") } else { format!("u:{user}:r") };
    StringCommand::new(vec![
        CommandBit::from("setfacl"),
        CommandBit::from("-m"),
        CommandBit::quoted(permission),
        CommandBit::quoted(path.to_owned()),
    ])
}

fn copy_command(source: &str, destination: &str) -> StringCommand {
    StringCommand::new(vec![
        CommandBit::from("cp"),
        CommandBit::quoted(source.to_owned()),
        CommandBit::quoted(destination.to_owned()),
    ])
}

fn download_stage_command(source: &str, temporary: &str) -> StringCommand {
    StringCommand::new(vec![
        CommandBit::from("cp"),
        CommandBit::quoted(source.to_owned()),
        CommandBit::quoted(temporary.to_owned()),
        CommandBit::from("&&"),
        CommandBit::from("chmod"),
        CommandBit::from("+r"),
        CommandBit::quoted(temporary.to_owned()),
    ])
}

fn remove_command(path: &str) -> StringCommand {
    StringCommand::new(vec![CommandBit::from("rm"), CommandBit::from("-f"), CommandBit::quoted(path.to_owned())])
}

fn require_success(result: &CommandResult, action: &'static str) -> Result<(), SshConnectorError> {
    if result.success {
        Ok(())
    } else {
        Err(SshConnectorError::TransferCommand { action, stderr: result.output.stderr() })
    }
}

fn is_nfs_v4_acl_mismatch(stderr: &str) -> bool {
    stderr.contains("branding mismatch") && stderr.contains("existing ACL is NFSv4")
}

#[cfg(test)]
mod tests {
    use super::{is_nfs_v4_acl_mismatch, temporary_filename, without_privilege};
    use crate::connectors::RunOptions;

    #[test]
    fn temporary_filename_matches_pyinfra_sha1_path_and_honors_override() {
        let options = RunOptions::default();

        assert_eq!(
            temporary_filename("not-another-file", None, &options),
            "/tmp/pyinfra-43db9984686317089fefcf2e38de527e4cb44487"
        );
        assert_eq!(temporary_filename("ignored", Some("/custom"), &options), "/custom");
    }

    #[test]
    fn no_privilege_options_remove_escalation_and_working_directory() {
        let mut options = RunOptions::default();
        options.unix.sudo = true;
        options.unix.su_user = Some("deploy".to_owned());
        options.unix.working_directory = Some("/root".to_owned());

        let plain = without_privilege(&options);

        assert!(!plain.unix.sudo);
        assert_eq!(plain.unix.su_user, None);
        assert_eq!(plain.unix.working_directory, None);
    }

    #[test]
    fn freebsd_nfs_v4_acl_error_is_detected_by_both_reference_fragments() {
        assert!(is_nfs_v4_acl_mismatch("branding mismatch; existing ACL is NFSv4"));
        assert!(!is_nfs_v4_acl_mismatch("branding mismatch only"));
    }
}

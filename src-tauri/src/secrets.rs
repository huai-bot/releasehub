//! Secrets never live in the portable database or exports.
use std::{fs, io::Write, path::Path};
use uuid::Uuid;

#[derive(Clone)]
pub struct SecretStore {
    instance_id: String,
}

impl SecretStore {
    pub fn for_data_directory(directory: &Path) -> Result<Self, String> {
        let path = directory.join("instance-id");
        let instance_id = if path.exists() {
            let text = fs::read_to_string(&path).map_err(|_| "无法读取数据目录的身份标记。")?;
            Uuid::parse_str(text.trim())
                .map_err(|_| "数据目录的身份标记损坏，请先保留数据并检查目录。")?
                .to_string()
        } else {
            let id = Uuid::new_v4().to_string();
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|_| "数据目录不可写，请将便携文件夹移到可写的位置。")?;
            file.write_all(id.as_bytes()).map_err(|_| "无法保存数据目录的身份标记。")?;
            id
        };
        Ok(Self { instance_id })
    }

    /// Connectors may read an existing identity, but never create an application data directory.
    pub fn open_existing(directory: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(directory.join("instance-id"))
            .map_err(|_| "请先打开 ReleaseHub，并在设置中开启外部 AI 连接。")?;
        let instance_id = Uuid::parse_str(text.trim())
            .map_err(|_| "ReleaseHub 数据目录的身份标记无效。")?.to_string();
        Ok(Self { instance_id })
    }

    pub fn instance_id(&self) -> &str { &self.instance_id }

    #[cfg(windows)]
    fn entry(&self, name: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new("ReleaseHub", &format!("{}:{name}", self.instance_id))
            .map_err(|_| "无法访问 Windows 凭据管理器。".to_string())
    }

    #[cfg(windows)]
    pub fn get(&self, name: &str) -> Result<Option<String>, String> {
        match self.entry(name)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("无法读取本机保存的授权，请在设置中重新连接。".into()),
        }
    }

    #[cfg(windows)]
    pub fn set(&self, name: &str, value: Option<&str>) -> Result<(), String> {
        let entry = self.entry(name)?;
        match value.filter(|value| !value.is_empty()) {
            Some(value) => entry.set_password(value).map_err(|_| "无法保存到 Windows 凭据管理器。".into()),
            None => match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(_) => Err("无法删除本机保存的授权。".into()),
            },
        }
    }

    #[cfg(not(windows))]
    pub fn get(&self, _name: &str) -> Result<Option<String>, String> {
        let _ = &self.instance_id;
        Ok(None)
    }

    #[cfg(not(windows))]
    pub fn set(&self, _name: &str, _value: Option<&str>) -> Result<(), String> {
        Err("此版本仅在 Windows 上保存账户授权。".into())
    }
}

pub fn random_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_identity_is_stable_without_storing_secrets() {
        let directory = tempfile::tempdir().unwrap();
        let first = SecretStore::for_data_directory(directory.path()).unwrap();
        let second = SecretStore::for_data_directory(directory.path()).unwrap();
        assert_eq!(first.instance_id, second.instance_id);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        let contents = fs::read_to_string(directory.path().join("instance-id")).unwrap();
        assert!(Uuid::parse_str(&contents).is_ok());
    }

    #[test]
    fn tokens_are_distinct_and_have_fixed_length() {
        let a = random_token();
        let b = random_token();
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}

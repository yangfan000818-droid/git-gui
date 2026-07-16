use keyring::{Entry, Error as KeyringError};

const SERVICE: &str = "com.gitgui.app";
const AI_API_KEY_ACCOUNT: &str = "ai-api-key";

pub(crate) trait CredentialStore: Send + Sync {
    fn get_ai_api_key(&self) -> Result<Option<String>, String>;
    fn set_ai_api_key(&self, value: &str) -> Result<(), String>;
    fn delete_ai_api_key(&self) -> Result<(), String>;
}

#[derive(Default)]
pub(crate) struct SystemCredentialStore;

impl SystemCredentialStore {
    fn entry(&self) -> Result<Entry, String> {
        Entry::new(SERVICE, AI_API_KEY_ACCOUNT).map_err(|e| format!("无法访问系统凭据存储:{e}"))
    }
}

impl CredentialStore for SystemCredentialStore {
    fn get_ai_api_key(&self) -> Result<Option<String>, String> {
        match self.entry()?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(e) => Err(format!("读取系统凭据失败:{e}")),
        }
    }

    fn set_ai_api_key(&self, value: &str) -> Result<(), String> {
        self.entry()?
            .set_password(value)
            .map_err(|e| format!("写入系统凭据失败:{e}"))
    }

    fn delete_ai_api_key(&self) -> Result<(), String> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(e) => Err(format!("删除系统凭据失败:{e}")),
        }
    }
}

pub(crate) struct CredentialState(Box<dyn CredentialStore>);

impl CredentialState {
    pub(crate) fn store(&self) -> &dyn CredentialStore {
        self.0.as_ref()
    }
}

impl Default for CredentialState {
    fn default() -> Self {
        Self(Box::new(SystemCredentialStore))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::CredentialStore;
    use std::sync::Mutex;

    #[derive(Default)]
    pub(crate) struct MemoryCredentialStore {
        value: Mutex<Option<String>>,
        fail_get: Mutex<Option<String>>,
        fail_set: Mutex<Option<String>>,
        fail_delete: Mutex<Option<String>>,
    }

    impl MemoryCredentialStore {
        pub(crate) fn with_value(value: &str) -> Self {
            Self {
                value: Mutex::new(Some(value.to_string())),
                ..Default::default()
            }
        }

        pub(crate) fn value(&self) -> Option<String> {
            self.value.lock().unwrap().clone()
        }

        pub(crate) fn fail_get(&self, message: &str) {
            *self.fail_get.lock().unwrap() = Some(message.to_string());
        }

        pub(crate) fn fail_set(&self, message: &str) {
            *self.fail_set.lock().unwrap() = Some(message.to_string());
        }

        pub(crate) fn fail_delete(&self, message: &str) {
            *self.fail_delete.lock().unwrap() = Some(message.to_string());
        }
    }

    impl CredentialStore for MemoryCredentialStore {
        fn get_ai_api_key(&self) -> Result<Option<String>, String> {
            if let Some(message) = self.fail_get.lock().unwrap().clone() {
                return Err(message);
            }
            Ok(self.value())
        }

        fn set_ai_api_key(&self, value: &str) -> Result<(), String> {
            if let Some(message) = self.fail_set.lock().unwrap().clone() {
                return Err(message);
            }
            *self.value.lock().unwrap() = Some(value.to_string());
            Ok(())
        }

        fn delete_ai_api_key(&self) -> Result<(), String> {
            if let Some(message) = self.fail_delete.lock().unwrap().clone() {
                return Err(message);
            }
            *self.value.lock().unwrap() = None;
            Ok(())
        }
    }
}

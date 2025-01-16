use zeroize::Zeroize;
use anyhow::anyhow;

/// The credentials needed to encrypt and decrypt an encrypted file
#[derive(Clone, Default)]
pub struct Credentials {
    username: String,
    password: String,
    confirm_password: String,
}

impl Drop for Credentials {
    fn drop(&mut self) {
        self.erase();
    }
}

impl Credentials {
    pub fn new(username: String, password: String, confirm_password: String) -> Self {
        Self {
            username,
            password,
            confirm_password,
        }
    }

    /// Erase the credentials from memory by zeroizing the username and password
    pub fn erase(&mut self) {
        self.username.zeroize();
        self.password.zeroize();
        self.confirm_password.zeroize();
    }

    pub fn username(&self) -> &String {
        &self.username
    }

    pub fn password(&self) -> &String {
        &self.password
    }

    pub fn confirm_password(&self) -> &String {
        &self.confirm_password
    }

    /// Get a mutable reference to the username
    pub fn user_mut(&mut self) -> &mut String {
        &mut self.username
    }

    /// Get a mutable reference to the password
    pub fn passwd_mut(&mut self) -> &mut String {
        &mut self.password
    }

    /// Get a mutable reference to the confirm password
    pub fn confirm_passwd_mut(&mut self) -> &mut String {
        &mut self.confirm_password
    }

    /// Copy password to confirm password
    pub fn copy_passwd_to_confirm(&mut self) {
        self.confirm_password.clear();
        self.confirm_password.push_str(&self.password);
    }

    pub fn is_valid(&self) -> Result<(), anyhow::Error> {
        if self.username.is_empty() {
            return Err(anyhow!("Username must be provided"));
        }
        
        if self.password.is_empty() {
            return Err(anyhow!("Password must be provided"));
        }

        if self.confirm_password.is_empty() {
            return Err(anyhow!("Confirm password must be provided"));
        }

        if self.password != self.confirm_password {
            return Err(anyhow!("Passwords do not match"));
        }

        Ok(())
    }
}

#[cfg(test)]

mod tests {
    use super::*;

    #[test]
    fn test_credentials() {
        let mut credentials = Credentials::new(
            "test".to_string(),
            "password".to_string(),
            "password".to_string(),
        );
        assert!(credentials.is_valid().is_ok());


        credentials.erase();
        assert_eq!(credentials.username().is_empty(), true);
        assert_eq!(credentials.password().is_empty(), true);
        assert_eq!(credentials.confirm_password().is_empty(), true);

    }
}
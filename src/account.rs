use serde_json::Value;
use std::path::Path;

#[derive(serde::Serialize, Clone, Debug, PartialEq, Default)]
pub struct Account { pub email: String, pub org: String }

/// Read the Claude account identity from `<home>/.claude.json`. Best-effort:
/// returns None if the file/fields are absent.
pub fn read(home: &Path) -> Option<Account> {
    let raw = std::fs::read_to_string(home.join(".claude.json")).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let acc = v.get("oauthAccount")?;
    let email = acc.get("emailAddress").and_then(|x| x.as_str()).unwrap_or("").to_string();
    if email.is_empty() { return None; }
    let org = acc.get("organizationName").and_then(|x| x.as_str()).unwrap_or("").to_string();
    Some(Account { email, org })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_email_and_org() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(".claude.json"),
            r#"{"oauthAccount":{"emailAddress":"a@b.com","organizationName":"Acme"},"other":1}"#).unwrap();
        assert_eq!(read(d.path()), Some(Account { email: "a@b.com".into(), org: "Acme".into() }));
    }
    #[test]
    fn none_when_missing_file() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(read(d.path()), None);
    }
    #[test]
    fn none_when_no_email() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(".claude.json"), r#"{"oauthAccount":{"organizationName":"Acme"}}"#).unwrap();
        assert_eq!(read(d.path()), None);
    }
    #[test]
    fn org_optional() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(".claude.json"), r#"{"oauthAccount":{"emailAddress":"a@b.com"}}"#).unwrap();
        assert_eq!(read(d.path()).unwrap().org, "");
    }
}

use serde::{Deserialize, Serialize};

/// Canonical authority-cell identity. Namespace remains a separate partition.
/// Components are case-sensitive ASCII identifiers, not paths or display labels.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "ScopeFields")]
pub struct AuthorityScope {
    installation_id: String,
    tenant_id: String,
    workspace_id: String,
    environment_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeFields {
    installation_id: String,
    tenant_id: String,
    workspace_id: String,
    environment_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid authority scope: {0}")]
pub struct AuthorityScopeError(String);

impl AuthorityScope {
    pub fn new(
        installation_id: impl Into<String>,
        tenant_id: impl Into<String>,
        workspace_id: impl Into<String>,
        environment_id: impl Into<String>,
    ) -> Result<Self, AuthorityScopeError> {
        let scope = Self {
            installation_id: installation_id.into(),
            tenant_id: tenant_id.into(),
            workspace_id: workspace_id.into(),
            environment_id: environment_id.into(),
        };
        for component in [
            &scope.installation_id,
            &scope.tenant_id,
            &scope.workspace_id,
            &scope.environment_id,
        ] {
            if component.is_empty()
                || component.len() > 128
                || !component.as_bytes()[0].is_ascii_alphanumeric()
                || !component
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            {
                return Err(AuthorityScopeError(
                    "components must match [A-Za-z0-9][A-Za-z0-9._-]{0,127}".into(),
                ));
            }
        }
        Ok(scope)
    }

    pub fn installation_id(&self) -> &str {
        &self.installation_id
    }

    pub fn tenant_id(&self) -> &str {
        &self.tenant_id
    }

    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    pub fn environment_id(&self) -> &str {
        &self.environment_id
    }

    /// Normative workspace scope, intentionally excluding installation/namespace.
    pub fn workspace_scope(&self) -> String {
        format!(
            "{}/{}/{}",
            self.tenant_id, self.workspace_id, self.environment_id
        )
    }

    /// Versioned, collision-free key for the existing opaque storage/authz APIs.
    pub fn storage_key(&self) -> String {
        format!(
            "mesh-authority-v1/{}/{}",
            self.installation_id,
            self.workspace_scope()
        )
    }

    pub fn from_storage_key(key: &str) -> Result<Self, AuthorityScopeError> {
        let parts: Vec<_> = key.split('/').collect();
        match parts.as_slice() {
            [
                "mesh-authority-v1",
                installation,
                tenant,
                workspace,
                environment,
            ] => Self::new(*installation, *tenant, *workspace, *environment),
            _ => Err(AuthorityScopeError(
                "expected mesh-authority-v1/i/t/w/e".into(),
            )),
        }
    }
}

impl TryFrom<ScopeFields> for AuthorityScope {
    type Error = AuthorityScopeError;

    fn try_from(fields: ScopeFields) -> Result<Self, Self::Error> {
        Self::new(
            fields.installation_id,
            fields.tenant_id,
            fields.workspace_id,
            fields.environment_id,
        )
    }
}

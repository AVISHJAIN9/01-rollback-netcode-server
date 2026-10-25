/// Authorization module: roles, resource-level access control
use std::collections::HashMap;
use uuid::Uuid;

/// User roles
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Player,
    Admin,
}

impl Role {
    pub fn as_str(&self) -> &str {
        match self {
            Role::Player => "player",
            Role::Admin => "admin",
        }
    }
}

/// Resource type for authorization checks
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResourceType {
    Session(Uuid),
    AdminPanel,
    MatchResult,
}

/// Authorization check result
#[derive(Debug, Clone, PartialEq)]
pub enum AuthzResult {
    Allowed,
    Denied(String),
}

impl AuthzResult {
    pub fn is_allowed(&self) -> bool {
        matches!(self, AuthzResult::Allowed)
    }
}

/// Simple role-based access control
pub struct AuthorizationManager {
    permissions: HashMap<(Role, String), bool>,
}

impl AuthorizationManager {
    pub fn new() -> Self {
        let mut perms = HashMap::new();

        // Player permissions
        perms.insert((Role::Player, "join_session".to_string()), true);
        perms.insert((Role::Player, "read_own_session".to_string()), true);
        perms.insert((Role::Player, "leave_session".to_string()), true);
        perms.insert((Role::Player, "access_admin_panel".to_string()), false);
        perms.insert((Role::Player, "kick_player".to_string()), false);
        perms.insert((Role::Player, "ban_player".to_string()), false);

        // Admin permissions
        perms.insert((Role::Admin, "join_session".to_string()), true);
        perms.insert((Role::Admin, "read_own_session".to_string()), true);
        perms.insert((Role::Admin, "leave_session".to_string()), true);
        perms.insert((Role::Admin, "access_admin_panel".to_string()), true);
        perms.insert((Role::Admin, "kick_player".to_string()), true);
        perms.insert((Role::Admin, "ban_player".to_string()), true);
        perms.insert((Role::Admin, "terminate_session".to_string()), true);
        perms.insert((Role::Admin, "view_audit_log".to_string()), true);

        AuthorizationManager { permissions: perms }
    }

    /// Check if role has permission for an action
    pub fn can_perform(&self, role: Role, action: &str) -> bool {
        self.permissions
            .get(&(role, action.to_string()))
            .copied()
            .unwrap_or(false)
    }

    /// Check if a player can access a session (must be owner or admin)
    pub fn can_access_session(
        &self,
        role: Role,
        requester_id: &str,
        session_owner_id: &str,
    ) -> AuthzResult {
        match role {
            Role::Admin => AuthzResult::Allowed,
            Role::Player if requester_id == session_owner_id => AuthzResult::Allowed,
            Role::Player => AuthzResult::Denied("Cannot access another player's session".to_string()),
        }
    }

    /// Check if a player can access admin endpoints
    pub fn can_access_admin_endpoint(&self, role: Role) -> AuthzResult {
        if role == Role::Admin {
            AuthzResult::Allowed
        } else {
            AuthzResult::Denied("Admin access required".to_string())
        }
    }

    /// Check if a player can modify a match result
    pub fn can_modify_match_result(
        &self,
        role: Role,
        requester_id: &str,
        match_owner_id: &str,
    ) -> AuthzResult {
        match role {
            Role::Admin => AuthzResult::Allowed,
            Role::Player if requester_id == match_owner_id => {
                AuthzResult::Denied("Players cannot modify match results".to_string())
            }
            Role::Player => {
                AuthzResult::Denied("Cannot access another player's match result".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player_cannot_kick() {
        let authz = AuthorizationManager::new();
        assert!(!authz.can_perform(Role::Player, "kick_player"));
    }

    #[test]
    fn test_admin_can_kick() {
        let authz = AuthorizationManager::new();
        assert!(authz.can_perform(Role::Admin, "kick_player"));
    }

    #[test]
    fn test_player_cannot_access_admin_panel() {
        let authz = AuthorizationManager::new();
        let result = authz.can_access_admin_endpoint(Role::Player);
        assert!(matches!(result, AuthzResult::Denied(_)));
    }

    #[test]
    fn test_admin_can_access_admin_panel() {
        let authz = AuthorizationManager::new();
        let result = authz.can_access_admin_endpoint(Role::Admin);
        assert_eq!(result, AuthzResult::Allowed);
    }

    #[test]
    fn test_player_can_access_own_session() {
        let authz = AuthorizationManager::new();
        let result = authz.can_access_session(Role::Player, "player-1", "player-1");
        assert_eq!(result, AuthzResult::Allowed);
    }

    #[test]
    fn test_player_cannot_access_other_session() {
        let authz = AuthorizationManager::new();
        let result = authz.can_access_session(Role::Player, "player-1", "player-2");
        assert!(matches!(result, AuthzResult::Denied(_)));
    }

    #[test]
    fn test_admin_can_access_any_session() {
        let authz = AuthorizationManager::new();
        let result = authz.can_access_session(Role::Admin, "admin-1", "player-1");
        assert_eq!(result, AuthzResult::Allowed);
    }

    #[test]
    fn test_player_cannot_modify_match_result() {
        let authz = AuthorizationManager::new();
        let result = authz.can_modify_match_result(Role::Player, "player-1", "player-1");
        assert!(matches!(result, AuthzResult::Denied(_)));
    }

    #[test]
    fn test_admin_can_modify_match_result() {
        let authz = AuthorizationManager::new();
        let result = authz.can_modify_match_result(Role::Admin, "admin-1", "player-1");
        assert_eq!(result, AuthzResult::Allowed);
    }
}

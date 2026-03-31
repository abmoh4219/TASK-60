//! Frontend auth context.
//!
//! `AuthContext` is a `UseReducerHandle<AuthState>` placed at the tree root by
//! `App`.  Any component can read it via `use_context::<AuthContext>()`.
//!
//! Tokens + user info are persisted in `sessionStorage` so they survive page
//! reloads within the same browser tab but are cleared when the tab closes.

use std::rc::Rc;

use gloo_storage::{SessionStorage, Storage};
use serde::{Deserialize, Serialize};
use yew::prelude::*;

use shared::UserRole;

// ── Current user (mirrors backend UserInfo) ───────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurrentUser {
    pub id:        String,
    pub username:  String,
    pub role:      UserRole,
    pub full_name: Option<String>,
}

impl CurrentUser {
    pub fn display_name(&self) -> &str {
        self.full_name.as_deref().unwrap_or(&self.username)
    }

    pub fn role_label(&self) -> &'static str {
        match self.role {
            UserRole::Admin      => "Administrator",
            UserRole::OpsAgent   => "Operations Agent",
            UserRole::CsAgent    => "Customer Service",
            UserRole::Dispatcher => "Dispatcher",
            UserRole::Kiosk      => "Kiosk",
        }
    }
}

// ── Auth state ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum AuthStatus {
    /// First render: checking sessionStorage / verifying token with /me.
    Loading,
    Authenticated(CurrentUser),
    Unauthenticated,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AuthState {
    pub status: AuthStatus,
    pub token:  Option<String>,
}

impl AuthState {
    /// Restore from sessionStorage on first render.
    pub fn load() -> Self {
        if let Ok(token) = SessionStorage::get::<String>("railops_token") {
            AuthState { status: AuthStatus::Loading, token: Some(token) }
        } else {
            AuthState { status: AuthStatus::Unauthenticated, token: None }
        }
    }

    pub fn is_authenticated(&self) -> bool {
        matches!(self.status, AuthStatus::Authenticated(_))
    }

    pub fn current_user(&self) -> Option<&CurrentUser> {
        match &self.status {
            AuthStatus::Authenticated(u) => Some(u),
            _ => None,
        }
    }
}

// ── Actions ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub enum AuthAction {
    /// Called after a successful /login or /me verification.
    SetUser { token: String, user: CurrentUser },
    /// Token verification failed; clear everything.
    ClearSession,
    /// Explicit logout.
    Logout,
}

impl Reducible for AuthState {
    type Action = AuthAction;

    fn reduce(self: Rc<Self>, action: Self::Action) -> Rc<Self> {
        match action {
            AuthAction::SetUser { token, user } => {
                let _ = SessionStorage::set("railops_token", &token);
                let _ = SessionStorage::set("railops_user", &user);
                Rc::new(AuthState { status: AuthStatus::Authenticated(user), token: Some(token) })
            }
            AuthAction::ClearSession | AuthAction::Logout => {
                SessionStorage::delete("railops_token");
                SessionStorage::delete("railops_user");
                Rc::new(AuthState { status: AuthStatus::Unauthenticated, token: None })
            }
        }
    }
}

pub type AuthContext = UseReducerHandle<AuthState>;

//! Role-based access control.
//!
//! Each handler that needs authorisation declares its required `Permission`
//! as a typed Actix extractor.  Extractors build on top of `AuthUser` so the
//! full auth + rate-limit pipeline runs first.

use actix_web::{dev::Payload, FromRequest, HttpRequest};
use futures::future::{ready, Ready};

use shared::UserRole;

use crate::error::AppError;

use super::middleware::AuthUser;

// ── Permission catalogue ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    // Content / kiosk
    ViewContent,
    ManageContent,

    // Scheduling & inventory
    ViewSchedules,
    ManageSchedules,

    // Orders
    ViewOrders,
    ManageOrders,    // rebook, cancel
    OverrideFees,    // fee waiver; requires reason note
    ProcessRefunds,

    // Staffing
    ViewContractors,
    ManageContractors,
    ManageShifts,

    // Credentials
    ViewCredentials,
    ApproveCredentials,

    // Data ingestion
    ManageCrawl,

    // Administration
    ManageUsers,
    ConfigureRules,
    ViewAuditLog,
}

/// Local extension trait — allows calling `.has(perm)` on `UserRole` from
/// inside this module (including macro expansions) without defining an inherent
/// impl on a type that belongs to the `shared` crate (which would be E0116).
trait RoleExt {
    fn has(&self, perm: Permission) -> bool;
}

impl RoleExt for UserRole {
    fn has(&self, perm: Permission) -> bool {
        use Permission::*;
        match self {
            UserRole::Admin => true, // all permissions

            UserRole::OpsAgent => matches!(
                perm,
                ViewContent
                | ManageContent
                | ViewSchedules
                | ManageSchedules
                | ViewOrders
                | ManageOrders
                | OverrideFees
                | ProcessRefunds
                | ViewContractors
                | ViewCredentials
                | ManageCrawl
                | ViewAuditLog
            ),

            UserRole::CsAgent => matches!(
                perm,
                ViewContent | ViewSchedules | ViewOrders | ManageOrders | ProcessRefunds
            ),

            UserRole::Dispatcher => matches!(
                perm,
                ViewContent
                | ViewSchedules
                | ViewContractors
                | ManageContractors
                | ManageShifts
                | ViewCredentials
                | ApproveCredentials
                | ViewAuditLog
            ),

            UserRole::Kiosk => matches!(perm, ViewContent),
        }
    }
}

// ── Typed permission extractors ───────────────────────────────────────────────
// Each guard is a newtype around AuthUser.  Handlers declare the guard they need
// as a parameter; Actix calls from_request which enforces the role check.

macro_rules! permission_guard {
    ($guard:ident, $perm:expr) => {
        /// Actix extractor that requires `AuthUser` + specific permission.
        pub struct $guard(pub AuthUser);

        impl std::ops::Deref for $guard {
            type Target = AuthUser;
            fn deref(&self) -> &AuthUser { &self.0 }
        }

        impl FromRequest for $guard {
            type Error = AppError;
            type Future = futures::future::LocalBoxFuture<'static, Result<Self, AppError>>;

            fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
                let fut = AuthUser::from_request(req, payload);
                Box::pin(async move {
                    let user = fut.await?;
                    if !user.role.has($perm) {
                        return Err(AppError::Forbidden(format!(
                            "Role `{}` lacks permission `{:?}`",
                            user.role.as_str(), $perm
                        )));
                    }
                    Ok($guard(user))
                })
            }
        }
    };
}

permission_guard!(RequireViewContent,       Permission::ViewContent);
permission_guard!(RequireManageContent,     Permission::ManageContent);
permission_guard!(RequireViewSchedules,     Permission::ViewSchedules);
permission_guard!(RequireManageSchedules,   Permission::ManageSchedules);
permission_guard!(RequireViewOrders,        Permission::ViewOrders);
permission_guard!(RequireManageOrders,      Permission::ManageOrders);
permission_guard!(RequireOverrideFees,      Permission::OverrideFees);
permission_guard!(RequireProcessRefunds,    Permission::ProcessRefunds);
permission_guard!(RequireViewContractors,   Permission::ViewContractors);
permission_guard!(RequireManageContractors, Permission::ManageContractors);
permission_guard!(RequireManageShifts,      Permission::ManageShifts);
permission_guard!(RequireViewCredentials,   Permission::ViewCredentials);
permission_guard!(RequireApproveCredentials,Permission::ApproveCredentials);
permission_guard!(RequireManageCrawl,       Permission::ManageCrawl);
permission_guard!(RequireManageUsers,       Permission::ManageUsers);
permission_guard!(RequireConfigureRules,    Permission::ConfigureRules);
permission_guard!(RequireViewAuditLog,      Permission::ViewAuditLog);

// ── Convenience: any authenticated non-kiosk staff member ────────────────────

pub struct StaffUser(pub AuthUser);

impl std::ops::Deref for StaffUser {
    type Target = AuthUser;
    fn deref(&self) -> &AuthUser { &self.0 }
}

impl FromRequest for StaffUser {
    type Error = AppError;
    type Future = futures::future::LocalBoxFuture<'static, Result<Self, AppError>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let fut = AuthUser::from_request(req, payload);
        Box::pin(async move {
            let user = fut.await?;
            if user.role == UserRole::Kiosk {
                return Err(AppError::Forbidden("Kiosk access not permitted here".into()));
            }
            Ok(StaffUser(user))
        })
    }
}

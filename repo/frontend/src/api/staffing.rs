//! Typed API client for the staffing console endpoints.

use serde::{Deserialize, Serialize};

use super::{parse_json, signed_request, ApiResult};

// ── Re-use the pagination wrapper from ops ────────────────────────────────────
pub use crate::api::ops::Page;

// ── Shift types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ShiftRow {
    pub id:            String,
    pub role:          String,
    pub region:        Option<String>,
    pub required_tags: Vec<String>,
    pub shift_start:   String,
    pub shift_end:     String,
    pub status:        String,
    pub is_critical:   bool,
    pub created_at:    String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct AssignmentRow {
    pub id:              String,
    pub shift_id:        String,
    pub status:          String,
    /// Decimal serialised as a JSON string by the backend.
    pub match_score:     Option<String>,
    /// JSONB array of rationale strings.
    pub match_reasons:   Option<Vec<String>>,
    pub assigned_at:     String,
    pub contractor_id:   String,
    pub contractor_name: String,
    pub region:          Option<String>,
    pub quality_rating:  String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ShiftDetail {
    pub shift:       ShiftRow,
    pub assignments: Vec<AssignmentRow>,
}

// ── Contractor types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ContractorRow {
    pub id:             String,
    pub full_name:      String,
    pub region:         Option<String>,
    pub quality_rating: String,
    pub is_active:      bool,
    pub created_at:     String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ContractorDetail {
    pub contractor: ContractorRow,
    pub tags:       Vec<String>,
}

// ── Match candidate types ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MatchCandidateItem {
    pub contractor_id: String,
    pub full_name:     String,
    pub score:         String,
    pub reasons:       [String; 3],
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CandidatesResponse {
    pub candidates: Vec<MatchCandidateItem>,
}

// ── Subscription types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SubscriptionItem {
    pub id:              String,
    pub subscriber_type: String,
    pub subscriber_id:   String,
    pub target_type:     String,
    pub target_id:       String,
    pub created_at:      String,
}

// ── Request body types ────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ProposeBody {
    pub contractor_id: String,
}

#[derive(Debug, Serialize)]
pub struct RespondBody {
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct SubscribeBody {
    pub subscriber_type: String,
    pub target_type:     String,
    pub target_id:       String,
}

// ── API functions — shifts ────────────────────────────────────────────────────

pub async fn list_shifts(
    token:   &str,
    status:  &str,
    region:  &str,
    role:    &str,
    page:    i64,
) -> ApiResult<Page<ShiftRow>> {
    let mut qs = format!("?page={page}&per_page=20");
    if !status.is_empty() { qs.push_str(&format!("&status={status}")); }
    if !region.is_empty() { qs.push_str(&format!("&region={region}")); }
    if !role.is_empty()   { qs.push_str(&format!("&role={role}"));     }
    let path = format!("/api/v1/staffing/shifts{qs}");
    let resp = signed_request("GET", &path, token, None::<&()>).await?;
    parse_json(resp).await
}

pub async fn get_shift(token: &str, id: &str) -> ApiResult<ShiftDetail> {
    let resp = signed_request(
        "GET", &format!("/api/v1/staffing/shifts/{id}"), token, None::<&()>,
    ).await?;
    parse_json(resp).await
}

pub async fn create_shift(token: &str, body: &serde_json::Value) -> ApiResult<serde_json::Value> {
    let resp = signed_request("POST", "/api/v1/staffing/shifts", token, Some(body)).await?;
    parse_json(resp).await
}

pub async fn update_shift_status(
    token:  &str,
    id:     &str,
    status: &str,
) -> ApiResult<serde_json::Value> {
    let body = serde_json::json!({ "status": status });
    let resp = signed_request(
        "PATCH", &format!("/api/v1/staffing/shifts/{id}/status"), token, Some(&body),
    ).await?;
    parse_json(resp).await
}

pub async fn get_candidates(token: &str, shift_id: &str) -> ApiResult<CandidatesResponse> {
    let resp = signed_request(
        "GET",
        &format!("/api/v1/staffing/shifts/{shift_id}/candidates"),
        token,
        None::<&()>,
    ).await?;
    parse_json(resp).await
}

pub async fn propose_assignment(
    token:         &str,
    shift_id:      &str,
    contractor_id: &str,
) -> ApiResult<serde_json::Value> {
    let body = ProposeBody { contractor_id: contractor_id.to_owned() };
    let resp = signed_request(
        "POST",
        &format!("/api/v1/staffing/shifts/{shift_id}/propose"),
        token,
        Some(&body),
    ).await?;
    parse_json(resp).await
}

pub async fn respond_assignment(
    token:  &str,
    id:     &str,
    status: &str,
) -> ApiResult<serde_json::Value> {
    let body = RespondBody { status: status.to_owned() };
    let resp = signed_request(
        "PATCH",
        &format!("/api/v1/staffing/assignments/{id}/respond"),
        token,
        Some(&body),
    ).await?;
    parse_json(resp).await
}

// ── API functions — contractors ───────────────────────────────────────────────

pub async fn list_contractors(
    token:     &str,
    region:    &str,
    is_active: Option<bool>,
    page:      i64,
) -> ApiResult<Page<ContractorRow>> {
    let mut qs = format!("?page={page}&per_page=20");
    if !region.is_empty() { qs.push_str(&format!("&region={region}")); }
    if let Some(a) = is_active { qs.push_str(&format!("&is_active={a}")); }
    let path = format!("/api/v1/staffing/contractors{qs}");
    let resp = signed_request("GET", &path, token, None::<&()>).await?;
    parse_json(resp).await
}

pub async fn get_contractor(token: &str, id: &str) -> ApiResult<ContractorDetail> {
    let resp = signed_request(
        "GET", &format!("/api/v1/staffing/contractors/{id}"), token, None::<&()>,
    ).await?;
    parse_json(resp).await
}

pub async fn set_contractor_active(
    token:  &str,
    id:     &str,
    active: bool,
) -> ApiResult<serde_json::Value> {
    let body = serde_json::json!({ "active": active });
    let resp = signed_request(
        "PATCH",
        &format!("/api/v1/staffing/contractors/{id}/active"),
        token,
        Some(&body),
    ).await?;
    parse_json(resp).await
}

// ── API functions — subscriptions ─────────────────────────────────────────────

pub async fn list_subscriptions(token: &str) -> ApiResult<Vec<SubscriptionItem>> {
    let resp = signed_request(
        "GET", "/api/v1/staffing/subscriptions", token, None::<&()>,
    ).await?;
    parse_json(resp).await
}

pub async fn subscribe(
    token:           &str,
    subscriber_type: &str,
    target_type:     &str,
    target_id:       &str,
) -> ApiResult<serde_json::Value> {
    let body = SubscribeBody {
        subscriber_type: subscriber_type.to_owned(),
        target_type:     target_type.to_owned(),
        target_id:       target_id.to_owned(),
    };
    let resp = signed_request("POST", "/api/v1/staffing/subscriptions", token, Some(&body)).await?;
    parse_json(resp).await
}

pub async fn unsubscribe(
    token:           &str,
    subscriber_type: &str,
    target_type:     &str,
    target_id:       &str,
) -> ApiResult<serde_json::Value> {
    let body = SubscribeBody {
        subscriber_type: subscriber_type.to_owned(),
        target_type:     target_type.to_owned(),
        target_id:       target_id.to_owned(),
    };
    let resp = signed_request(
        "DELETE", "/api/v1/staffing/subscriptions", token, Some(&body),
    ).await?;
    parse_json(resp).await
}

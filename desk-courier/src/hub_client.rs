//! The hub side of the shell (K1, K3, K5, K9): blocking HTTP with the
//! AR19 timeouts. All parsing and classification lives in courier-core;
//! this file only moves bytes.

use crate::config::{Config, POLL_WAIT_SECS};
use courier_core::hub::{HubMessage, parse_hub_response};
use courier_core::settle::{SettleCallOutcome, settle_call_outcome};
use std::io::Read;
use std::time::Duration;

/// Bound on any response body we read (the hub's envelope response for
/// a max-size payload plus headroom).
const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug)]
pub struct HubCallError {
    /// None = transport-level (unreachable); Some = HTTP status.
    pub status: Option<u16>,
    pub detail: String,
}

pub enum PolicyOutcome {
    AlreadyRight,
    /// Written; `overrode_explicit` = the PUT replaced a human's
    /// explicit dashboard edit (AR7: intended, but logged loudly).
    Written {
        overrode_explicit: bool,
    },
}

pub struct HubClient {
    poll: ureq::Agent,
    ops: ureq::Agent,
    base: String,
    topic: String,
    subscription: String,
    auth: String,
}

impl HubClient {
    pub fn new(config: &Config) -> Self {
        let poll = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(POLL_WAIT_SECS + 10))
            .build();
        let ops = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(10))
            .build();
        HubClient {
            poll,
            ops,
            base: config.hub_url.clone(),
            topic: config.topic.clone(),
            subscription: config.subscription.clone(),
            auth: format!("Bearer {}", config.token),
        }
    }

    /// K1: one long-poll. `Ok(None)` = 204, an empty window — the
    /// normal state of a healthy queue (AR5). `from_beginning` is set
    /// on the first poll of a run (AR7 cold start): a fresh
    /// subscription otherwise misses everything published before it
    /// first polled (drill-proven); replay is idempotent, dedup and
    /// the staleness check make it safe.
    pub fn receive(
        &self,
        from_beginning: bool,
    ) -> Result<Option<(HubMessage, Option<String>)>, HubCallError> {
        let url = format!(
            "{}/t/{}/next?as={}&envelope=json&wait={}{}",
            self.base,
            self.topic,
            self.subscription,
            POLL_WAIT_SECS,
            if from_beginning {
                "&from=beginning"
            } else {
                ""
            }
        );
        let response = self
            .poll
            .get(&url)
            .set("authorization", &self.auth)
            .call()
            .map_err(map_err)?;
        if response.status() == 204 {
            return Ok(None);
        }
        let notice = response.header("mailbox-notice").map(str::to_string);
        let body = read_bounded(response)?;
        let message = parse_hub_response(&body).map_err(|e| HubCallError {
            status: Some(200),
            detail: format!("hub response unreadable: {}", e.0),
        })?;
        Ok(Some((message, notice)))
    }

    /// K3 / W5: ack or nack; the outcome table (AR5) decides what the
    /// caller does with a rejection — never retry-loop a settle call.
    pub fn settle(&self, id: &str, ack: bool, dead: bool) -> (SettleCallOutcome, String) {
        let url = if ack {
            format!(
                "{}/t/{}/ack/{}?as={}",
                self.base, self.topic, id, self.subscription
            )
        } else {
            format!(
                "{}/t/{}/nack/{}?as={}{}",
                self.base,
                self.topic,
                id,
                self.subscription,
                if dead { "&dead=true" } else { "" }
            )
        };
        match self.ops.post(&url).set("authorization", &self.auth).call() {
            Ok(r) => (settle_call_outcome(Some(r.status())), String::new()),
            Err(e) => {
                let err = map_err(e);
                (settle_call_outcome(err.status), err.detail)
            }
        }
    }

    /// K5 / AR7: assert the one policy field the courier owns. GET the
    /// explicit set; PUT {"ttl_ms": …} only when it differs.
    pub fn ensure_policy(&self, ttl_ms: u64) -> Result<PolicyOutcome, HubCallError> {
        let url = format!(
            "{}/api/t/{}/subs/{}/policy",
            self.base, self.topic, self.subscription
        );
        let current = self
            .ops
            .get(&url)
            .set("authorization", &self.auth)
            .call()
            .map_err(map_err)?;
        let body = read_bounded(current)?;
        let json: serde_json::Value = serde_json::from_slice(&body).map_err(|e| HubCallError {
            status: Some(200),
            detail: format!("policy response unreadable: {e}"),
        })?;
        let explicit = &json["explicit"];
        let ttl_right = explicit["ttl_ms"].as_u64() == Some(ttl_ms);
        let others_explicit = ["lease_ms", "max_attempts", "backoff_ms"]
            .iter()
            .any(|k| !explicit[*k].is_null());
        if ttl_right && !others_explicit {
            return Ok(PolicyOutcome::AlreadyRight);
        }
        self.ops
            .put(&url)
            .set("authorization", &self.auth)
            .set("content-type", "application/json")
            .send_string(&format!("{{\"ttl_ms\": {ttl_ms}}}"))
            .map_err(map_err)?;
        Ok(PolicyOutcome::Written {
            overrode_explicit: others_explicit || (!ttl_right && !explicit["ttl_ms"].is_null()),
        })
    }

    /// AR21: revive an archived subscription, loudly (caller logs).
    pub fn unarchive(&self) -> Result<(), HubCallError> {
        let url = format!(
            "{}/api/t/{}/subs/{}/unarchive",
            self.base, self.topic, self.subscription
        );
        self.ops
            .post(&url)
            .set("authorization", &self.auth)
            .call()
            .map_err(map_err)?;
        Ok(())
    }

    /// M8: publish one envelope (the send-test path and the drills).
    pub fn publish(&self, envelope_json: &str) -> Result<String, HubCallError> {
        let url = format!("{}/t/{}", self.base, self.topic);
        let response = self
            .ops
            .post(&url)
            .set("authorization", &self.auth)
            .set("content-type", "application/json")
            .send_string(envelope_json)
            .map_err(map_err)?;
        let body = read_bounded(response)?;
        let json: serde_json::Value = serde_json::from_slice(&body).map_err(|e| HubCallError {
            status: Some(201),
            detail: format!("publish response unreadable: {e}"),
        })?;
        Ok(json["id"].as_str().unwrap_or("?").to_string())
    }
}

fn map_err(error: ureq::Error) -> HubCallError {
    match error {
        ureq::Error::Status(code, response) => {
            let detail = response
                .into_string()
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect();
            HubCallError {
                status: Some(code),
                detail,
            }
        }
        ureq::Error::Transport(t) => HubCallError {
            status: None,
            detail: t.to_string(),
        },
    }
}

fn read_bounded(response: ureq::Response) -> Result<Vec<u8>, HubCallError> {
    let mut buf = Vec::new();
    response
        .into_reader()
        .take(MAX_RESPONSE_BYTES)
        .read_to_end(&mut buf)
        .map_err(|e| HubCallError {
            status: None,
            detail: format!("reading response body: {e}"),
        })?;
    Ok(buf)
}

//! Who may do what: the rules of the policies a user gets from their groups, asked once per request. `IAuthz` is the interface `controller`
//! depends on; `AuthzImp` is its implementation. The rules are read from the database (a policy someone wrote there by hand counts) and kept
//! for a few seconds; the admin page drops them at once when it changes anything.
//!
//! A break-glass account is not asked: it can do everything, so a policy that was deleted or written wrong never locks everybody out.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use tracing::warn;

use crate::database::IPolicyDAO;
use crate::model::{Grant, User};
use crate::policy::{self, Cap, Decision, Rule};

const KEEP: Duration = Duration::from_secs(5);

pub trait IAuthz: Send + Sync {
    fn decide(&self, user: &User, cap: Cap, path: &str) -> Decision;
    fn allowed(&self, user: &User, cap: Cap, path: &str) -> bool {
        self.decide(user, cap, path).allowed()
    }
    /// The rules the user is under, as the page shows them.
    fn grants(&self, user: &User) -> Vec<Grant>;
    /// Something about policies, groups or memberships changed: ask the database again.
    fn forget(&self);
}

/// `(policy name, rule)` for each rule of each policy.
type Rules = Arc<Vec<(String, Rule)>>;

pub struct AuthzImp {
    policies: Arc<dyn IPolicyDAO>,
    cache: Mutex<HashMap<i64, (Instant, Rules)>>,
}

impl AuthzImp {
    pub fn new(policies: Arc<dyn IPolicyDAO>) -> Self {
        Self {
            policies,
            cache: Mutex::default(),
        }
    }

    fn rules(&self, user: i64) -> Rules {
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((at, rules)) = cache.get(&user)
            && at.elapsed() < KEEP
        {
            return rules.clone();
        }
        let mut rules = Vec::new();
        // a database that cannot be read, or a policy that cannot be parsed, gives no rules: refused, never allowed by mistake
        for (name, text) in self.policies.policies_of_user(user).unwrap_or_default() {
            match policy::parse(&text) {
                Ok(parsed) => rules.extend(parsed.into_iter().map(|r| (name.clone(), r))),
                Err(e) => warn!("policy {name} is ignored: {e}"),
            }
        }
        let rules = Arc::new(rules);
        cache.insert(user, (Instant::now(), rules.clone()));
        rules
    }
}

impl IAuthz for AuthzImp {
    fn decide(&self, user: &User, cap: Cap, path: &str) -> Decision {
        if user.break_glass {
            return Decision::Allowed {
                by: vec!["break-glass".into()],
            };
        }
        let rules = self.rules(user.id);
        policy::decide(rules.iter().map(|(n, r)| (n.as_str(), r)), cap, path)
    }

    fn grants(&self, user: &User) -> Vec<Grant> {
        if user.break_glass {
            return vec![Grant {
                path: "*".into(),
                capabilities: Cap::ALL
                    .iter()
                    .filter(|c| **c != Cap::Deny)
                    .map(|c| c.name().to_string())
                    .collect(),
            }];
        }
        self.rules(user.id)
            .iter()
            .map(|(_, r)| Grant {
                path: r.path.clone(),
                capabilities: r.caps.iter().map(|c| c.name().to_string()).collect(),
            })
            .collect()
    }

    fn forget(&self) {
        self.cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

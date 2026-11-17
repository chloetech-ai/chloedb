//! One GH Archive event, read from either of its two formats.
//!
//! From 2015 an event names its actor and repo by id (`actor.id`,
//! `repo.id`, `repo.name`). Before 2015 (the timeline API's format) the
//! actor is a login with no id, the repo is a `repository` object with its
//! id, owner, name, description, language and star count, and times carry
//! a UTC offset. Payloads differ by type and by year, and since about
//! October 2025 some are slimmed (pushes without sizes, pull requests
//! without their base repo), so every field here is optional and lenient:
//! a value of an unexpected type reads as missing rather than failing the
//! event.
//!
//! Nothing about people beyond their public login is read: not names,
//! emails, locations or companies (the old format's `actor_attributes`
//! is never looked at).

use serde::de::{self, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use std::fmt;

#[derive(Deserialize, Default)]
pub struct Event {
    #[serde(rename = "type", default, deserialize_with = "lenient_str")]
    pub ty: Option<String>,
    #[serde(default, deserialize_with = "lenient_str")]
    pub created_at: Option<String>,
    #[serde(default)]
    pub actor: Option<Actor>,
    #[serde(default)]
    pub repo: Option<RepoRef>,
    #[serde(default)]
    pub repository: Option<RepoMeta>,
    #[serde(default)]
    pub payload: Option<Payload>,
}

/// `actor`: an object with an id and login (2015 on), or a login (before).
#[derive(Default)]
pub struct Actor {
    pub id: Option<i64>,
    pub login: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct RepoRef {
    #[serde(default, deserialize_with = "lenient_i64")]
    pub id: Option<i64>,
    #[serde(default, deserialize_with = "lenient_str")]
    pub name: Option<String>,
}

/// A repository object: the old format's `repository`, a pull request's
/// `base.repo`, a fork's `forkee`. Field names differ between them.
#[derive(Deserialize, Default)]
pub struct RepoMeta {
    #[serde(default, deserialize_with = "lenient_i64")]
    pub id: Option<i64>,
    #[serde(default, deserialize_with = "lenient_str")]
    pub full_name: Option<String>,
    #[serde(default, deserialize_with = "lenient_str")]
    pub name: Option<String>,
    /// The old format's owner login (an object elsewhere, read as missing).
    #[serde(default, deserialize_with = "lenient_str")]
    pub owner: Option<String>,
    #[serde(default, deserialize_with = "lenient_str")]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "lenient_str")]
    pub language: Option<String>,
    #[serde(default, deserialize_with = "lenient_str")]
    pub homepage: Option<String>,
    #[serde(default, deserialize_with = "lenient_i64")]
    pub stargazers_count: Option<i64>,
    #[serde(default, deserialize_with = "lenient_i64")]
    pub stargazers: Option<i64>,
    #[serde(default, deserialize_with = "lenient_i64")]
    pub watchers: Option<i64>,
    #[serde(default, deserialize_with = "lenient_i64")]
    pub forks_count: Option<i64>,
    #[serde(default, deserialize_with = "lenient_i64")]
    pub forks: Option<i64>,
    #[serde(default, deserialize_with = "lenient_str")]
    pub created_at: Option<String>,
    #[serde(default, deserialize_with = "lenient_strs")]
    pub topics: Option<Vec<String>>,
    #[serde(default, deserialize_with = "lenient_license")]
    pub license: Option<String>,
    #[serde(default, deserialize_with = "lenient_bool")]
    pub fork: Option<bool>,
    #[serde(default, deserialize_with = "lenient_bool")]
    pub archived: Option<bool>,
}

impl RepoMeta {
    /// "owner/name", from whichever fields this format has.
    pub fn full(&self) -> Option<String> {
        if let Some(f) = &self.full_name {
            return Some(f.clone());
        }
        match (&self.owner, &self.name) {
            (Some(o), Some(n)) => Some(format!("{o}/{n}")),
            _ => None,
        }
    }
    pub fn stars(&self) -> Option<i64> {
        self.stargazers_count.or(self.stargazers).or(self.watchers)
    }
    pub fn fork_count(&self) -> Option<i64> {
        self.forks_count.or(self.forks)
    }
    /// Whether it says anything worth keeping beyond a name.
    pub fn has_details(&self) -> bool {
        self.description.is_some()
            || self.language.is_some()
            || self.stars().is_some()
            || self.topics.is_some()
            || self.license.is_some()
    }
}

#[derive(Deserialize, Default)]
pub struct Payload {
    #[serde(default, deserialize_with = "lenient_str")]
    pub action: Option<String>,
    #[serde(default, deserialize_with = "lenient_i64")]
    pub number: Option<i64>,
    #[serde(default, deserialize_with = "lenient_i64")]
    pub size: Option<i64>,
    #[serde(default, deserialize_with = "lenient_str")]
    pub ref_type: Option<String>,
    /// A CreateEvent's repository description.
    #[serde(default, deserialize_with = "lenient_str")]
    pub description: Option<String>,
    /// An issue: an object with a number (2015 on) or an id (before).
    #[serde(default, deserialize_with = "lenient_number")]
    pub issue: Option<i64>,
    #[serde(default)]
    pub pull_request: Option<PullRequest>,
    #[serde(default, deserialize_with = "lenient_id")]
    pub forkee: Option<i64>,
}

#[derive(Deserialize, Default)]
pub struct PullRequest {
    #[serde(default, deserialize_with = "lenient_i64")]
    pub number: Option<i64>,
    #[serde(default, deserialize_with = "lenient_bool")]
    pub merged: Option<bool>,
    #[serde(default)]
    pub base: Option<Base>,
}

#[derive(Deserialize, Default)]
pub struct Base {
    #[serde(default)]
    pub repo: Option<RepoMeta>,
}

impl<'de> Deserialize<'de> for Actor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Actor;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an actor")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Actor, E> {
                Ok(Actor {
                    id: None,
                    login: Some(v.to_owned()),
                })
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Actor, A::Error> {
                let mut a = Actor::default();
                while let Some(k) = m.next_key::<std::borrow::Cow<str>>()? {
                    match k.as_ref() {
                        "id" => a.id = m.next_value::<Lenient<i64>>()?.0,
                        "login" => a.login = m.next_value::<Lenient<String>>()?.0,
                        _ => {
                            m.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(a)
            }
            fn visit_unit<E: de::Error>(self) -> Result<Actor, E> {
                Ok(Actor::default())
            }
            fn visit_none<E: de::Error>(self) -> Result<Actor, E> {
                Ok(Actor::default())
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Actor, E> {
                Ok(Actor::default())
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Actor, E> {
                Ok(Actor::default())
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Actor, E> {
                Ok(Actor::default())
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Actor, A::Error> {
                while s.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Actor::default())
            }
        }
        d.deserialize_any(V)
    }
}

/// A value read as `T` when it has `T`'s type, and as missing otherwise.
pub struct Lenient<T>(pub Option<T>);

macro_rules! ignore_rest {
    ($t:ty) => {
        fn visit_unit<E: de::Error>(self) -> Result<$t, E> {
            Ok(Lenient(None))
        }
        fn visit_none<E: de::Error>(self) -> Result<$t, E> {
            Ok(Lenient(None))
        }
        fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<$t, A::Error> {
            while m.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
            Ok(Lenient(None))
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<$t, A::Error> {
            while s.next_element::<IgnoredAny>()?.is_some() {}
            Ok(Lenient(None))
        }
    };
}

impl<'de> Deserialize<'de> for Lenient<i64> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Lenient<i64>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an integer")
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(Lenient(Some(v)))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(Lenient(i64::try_from(v).ok()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(Lenient(
                    (v.fract() == 0.0 && v.abs() < 9e15).then_some(v as i64),
                ))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(Lenient(v.parse().ok()))
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Ok(Lenient(None))
            }
            ignore_rest!(Lenient<i64>);
        }
        d.deserialize_any(V)
    }
}

impl<'de> Deserialize<'de> for Lenient<String> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Lenient<String>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a string")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(Lenient(Some(v.to_owned())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(Lenient(Some(v)))
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Ok(Lenient(None))
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Ok(Lenient(None))
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Ok(Lenient(None))
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Ok(Lenient(None))
            }
            ignore_rest!(Lenient<String>);
        }
        d.deserialize_any(V)
    }
}

impl<'de> Deserialize<'de> for Lenient<bool> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Lenient<bool>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a boolean")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(Lenient(Some(v)))
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Ok(Lenient(None))
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Ok(Lenient(None))
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Ok(Lenient(None))
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Ok(Lenient(None))
            }
            ignore_rest!(Lenient<bool>);
        }
        d.deserialize_any(V)
    }
}

fn lenient_i64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    Ok(Lenient::<i64>::deserialize(d)?.0)
}
fn lenient_str<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(Lenient::<String>::deserialize(d)?.0)
}
fn lenient_bool<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    Ok(Lenient::<bool>::deserialize(d)?.0)
}

/// An object's field `want` as an integer, or the value itself when it is
/// one (`issue` is an object with a `number` from 2015, an id before;
/// `forkee` an object with an `id`).
fn int_or_field<'de, D: Deserializer<'de>>(
    d: D,
    want: &'static str,
    bare: bool,
) -> Result<Option<i64>, D::Error> {
    struct V {
        want: &'static str,
        bare: bool,
    }
    impl<'de> Visitor<'de> for V {
        type Value = Option<i64>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("an object or an integer")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Option<i64>, A::Error> {
            let mut out = None;
            while let Some(k) = m.next_key::<std::borrow::Cow<str>>()? {
                if k == self.want {
                    out = m.next_value::<Lenient<i64>>()?.0;
                } else {
                    m.next_value::<IgnoredAny>()?;
                }
            }
            Ok(out)
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Option<i64>, E> {
            Ok(self.bare.then_some(v))
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Option<i64>, E> {
            Ok(if self.bare {
                i64::try_from(v).ok()
            } else {
                None
            })
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> Result<Option<i64>, E> {
            Ok(None)
        }
        fn visit_str<E: de::Error>(self, _: &str) -> Result<Option<i64>, E> {
            Ok(None)
        }
        fn visit_bool<E: de::Error>(self, _: bool) -> Result<Option<i64>, E> {
            Ok(None)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Option<i64>, E> {
            Ok(None)
        }
        fn visit_none<E: de::Error>(self) -> Result<Option<i64>, E> {
            Ok(None)
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Option<i64>, A::Error> {
            while s.next_element::<IgnoredAny>()?.is_some() {}
            Ok(None)
        }
    }
    d.deserialize_any(V { want, bare })
}

/// An issue's number; before 2015 the payload held the issue's id here,
/// which is not its number, so a bare integer reads as missing (the old
/// format puts the number in `payload.number`).
fn lenient_number<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    int_or_field(d, "number", false)
}
fn lenient_id<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    int_or_field(d, "id", true)
}

/// A license's SPDX id: an object `{spdx_id, key, name}`, or missing.
fn lenient_license<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = Option<String>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a license")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Option<String>, A::Error> {
            let (mut spdx, mut key) = (None, None);
            while let Some(k) = m.next_key::<std::borrow::Cow<str>>()? {
                match k.as_ref() {
                    "spdx_id" => spdx = m.next_value::<Lenient<String>>()?.0,
                    "key" => key = m.next_value::<Lenient<String>>()?.0,
                    _ => {
                        m.next_value::<IgnoredAny>()?;
                    }
                }
            }
            Ok(spdx.filter(|s| s != "NOASSERTION").or(key))
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Option<String>, E> {
            Ok(Some(v.to_owned()))
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> Result<Option<String>, E> {
            Ok(None)
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> Result<Option<String>, E> {
            Ok(None)
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> Result<Option<String>, E> {
            Ok(None)
        }
        fn visit_bool<E: de::Error>(self, _: bool) -> Result<Option<String>, E> {
            Ok(None)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Option<String>, E> {
            Ok(None)
        }
        fn visit_none<E: de::Error>(self) -> Result<Option<String>, E> {
            Ok(None)
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Option<String>, A::Error> {
            while s.next_element::<IgnoredAny>()?.is_some() {}
            Ok(None)
        }
    }
    d.deserialize_any(V)
}

/// A list of strings (topics), skipping anything else in it.
fn lenient_strs<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<String>>, D::Error> {
    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = Option<Vec<String>>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a list of strings")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Option<Vec<String>>, A::Error> {
            let mut v = Vec::new();
            while let Some(x) = s.next_element::<Lenient<String>>()? {
                if let Some(x) = x.0 {
                    v.push(x);
                }
            }
            Ok((!v.is_empty()).then_some(v))
        }
        fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Option<Vec<String>>, A::Error> {
            while m.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
            Ok(None)
        }
        fn visit_str<E: de::Error>(self, _: &str) -> Result<Option<Vec<String>>, E> {
            Ok(None)
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> Result<Option<Vec<String>>, E> {
            Ok(None)
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> Result<Option<Vec<String>>, E> {
            Ok(None)
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> Result<Option<Vec<String>>, E> {
            Ok(None)
        }
        fn visit_bool<E: de::Error>(self, _: bool) -> Result<Option<Vec<String>>, E> {
            Ok(None)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Option<Vec<String>>, E> {
            Ok(None)
        }
        fn visit_none<E: de::Error>(self) -> Result<Option<Vec<String>>, E> {
            Ok(None)
        }
    }
    d.deserialize_any(V)
}

/// Seconds since 1970 of an archive time: "2024-06-17T15:00:00Z" or, before
/// 2015, "2013-06-15T15:00:42-07:00".
pub fn unix(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.timestamp())
}

/// What happened, as one short word: `star` for a WatchEvent (GitHub's
/// name for starring), `pr_merged` for a pull request closed by merging,
/// `create_tag` for a CreateEvent of a tag, and so on.
pub fn kind(ty: &str, p: Option<&Payload>) -> String {
    let action = p.and_then(|p| p.action.as_deref()).unwrap_or("");
    match ty {
        "WatchEvent" => "star".into(),
        "ForkEvent" => "fork".into(),
        "PushEvent" => "push".into(),
        "CreateEvent" => format!(
            "create_{}",
            p.and_then(|p| p.ref_type.as_deref()).unwrap_or("x")
        ),
        "DeleteEvent" => format!(
            "delete_{}",
            p.and_then(|p| p.ref_type.as_deref()).unwrap_or("x")
        ),
        "IssueCommentEvent" => "comment".into(),
        "PullRequestReviewEvent" => "review".into(),
        "PullRequestReviewCommentEvent" => "review_comment".into(),
        "CommitCommentEvent" => "commit_comment".into(),
        "ReleaseEvent" => "release".into(),
        "MemberEvent" => "member".into(),
        "PublicEvent" => "public".into(),
        "GollumEvent" => "wiki".into(),
        "DiscussionEvent" => "discussion".into(),
        "PullRequestEvent" => {
            let merged = p
                .and_then(|p| p.pull_request.as_ref())
                .and_then(|r| r.merged)
                .unwrap_or(false);
            if action == "closed" && merged {
                "pr_merged".into()
            } else {
                format!("pr_{}", if action.is_empty() { "x" } else { action })
            }
        }
        "IssuesEvent" => format!("issue_{}", if action.is_empty() { "x" } else { action }),
        other => {
            // FollowEvent, GistEvent, DownloadEvent ... before 2015; any
            // type GitHub adds later: its name without "Event", lowercased.
            let base = other.strip_suffix("Event").unwrap_or(other);
            let mut s = String::with_capacity(base.len() + 4);
            for (i, c) in base.chars().enumerate() {
                if c.is_ascii_uppercase() {
                    if i > 0 {
                        s.push('_');
                    }
                    s.push(c.to_ascii_lowercase());
                } else if c.is_ascii_alphanumeric() {
                    s.push(c);
                }
            }
            if s.is_empty() {
                "unknown".into()
            } else {
                s
            }
        }
    }
}

/// The number an event carries: a pull request's or issue's number, a
/// push's commit count, the id of a fork's new repo.
pub fn number(ty: &str, p: Option<&Payload>) -> Option<i64> {
    let p = p?;
    match ty {
        "PushEvent" => p.size,
        "ForkEvent" => p.forkee,
        "PullRequestEvent" | "PullRequestReviewEvent" | "PullRequestReviewCommentEvent" => {
            p.pull_request.as_ref().and_then(|r| r.number).or(p.number)
        }
        "IssuesEvent" | "IssueCommentEvent" => p.issue.or(p.number),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_formats() {
        let new = r#"{"id":"1","type":"PullRequestEvent","actor":{"id":5,"login":"ann","display_login":"ann"},"repo":{"id":9,"name":"o/r","url":"x"},"payload":{"action":"closed","number":12,"pull_request":{"number":12,"merged":true,"base":{"repo":{"id":9,"full_name":"o/r","description":"A thing","language":"Rust","stargazers_count":"bad","topics":["db",3],"license":{"spdx_id":"MIT"}}}}},"created_at":"2024-06-17T15:00:00Z"}"#;
        let e: Event = serde_json::from_str(new).unwrap();
        assert_eq!(e.actor.as_ref().unwrap().id, Some(5));
        assert_eq!(
            kind(e.ty.as_deref().unwrap(), e.payload.as_ref()),
            "pr_merged"
        );
        assert_eq!(number("PullRequestEvent", e.payload.as_ref()), Some(12));
        let base = e
            .payload
            .as_ref()
            .unwrap()
            .pull_request
            .as_ref()
            .unwrap()
            .base
            .as_ref()
            .unwrap()
            .repo
            .as_ref()
            .unwrap();
        assert_eq!(base.full().as_deref(), Some("o/r"));
        assert_eq!(base.stars(), None);
        assert_eq!(base.topics.as_deref(), Some(&["db".to_string()][..]));
        assert_eq!(base.license.as_deref(), Some("MIT"));
        assert_eq!(unix(e.created_at.as_deref().unwrap()), Some(1718636400));

        let old = r#"{"created_at":"2013-06-15T15:00:42-07:00","payload":{"action":"opened","issue":12345678,"number":7},"type":"IssuesEvent","actor":"bob","actor_attributes":{"login":"bob","email":"x@y"},"repository":{"id":10637273,"name":"DiffEdit","owner":"Lannyland","description":"A tool","watchers":3,"stargazers":4,"language":"C#","created_at":"2013-06-11T23:01:16-07:00"}}"#;
        let e: Event = serde_json::from_str(old).unwrap();
        assert_eq!(e.actor.as_ref().unwrap().login.as_deref(), Some("bob"));
        assert_eq!(e.actor.as_ref().unwrap().id, None);
        assert_eq!(number("IssuesEvent", e.payload.as_ref()), Some(7));
        let r = e.repository.as_ref().unwrap();
        assert_eq!(r.full().as_deref(), Some("Lannyland/DiffEdit"));
        assert_eq!(r.stars(), Some(4));
        assert_eq!(unix("2013-06-15T15:00:42-07:00"), Some(1371333642));
        assert_eq!(kind("FollowEvent", None), "follow");
        assert_eq!(
            kind("PullRequestReviewThreadEvent", None),
            "pull_request_review_thread"
        );
    }
}

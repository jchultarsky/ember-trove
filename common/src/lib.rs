/// Canonical wire strings for a fieldless enum serialized as serde
/// `snake_case`: generates `ALL`, `as_str` and `parse` from one table, so the
/// api (SQL enum casts, backups), the ui and the DTOs share a single mapping.
/// They used to be hand-written about 25 times across the crates, plus
/// `format!("{:?}").to_lowercase()` tricks. `wire_tests` pins every table to
/// serde.
macro_rules! wire_enum {
    ($ty:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        impl $ty {
            /// Every variant, in declaration order.
            pub const ALL: &'static [$ty] = &[$($ty::$variant),+];

            /// The canonical wire string — identical to serde's form.
            pub fn as_str(&self) -> &'static str {
                match self {
                    $($ty::$variant => $s),+
                }
            }

            /// Parse the wire string; `None` for anything unknown (callers
            /// choose their own fallback or error).
            pub fn parse(s: &str) -> Option<$ty> {
                match s {
                    $($s => Some($ty::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

pub mod activity;
pub mod admin;
pub mod attachment;
pub mod auth;
pub mod backup;
pub mod edge;
pub mod editor_pref;
pub mod error;
pub mod favorite;
pub mod graph;
pub mod graph_layout;
pub mod id;
pub mod inbox;
pub mod keyboard;
pub mod markdown;
pub mod node;
pub mod node_link;
pub mod node_version;
pub mod note;
pub mod permission;
pub mod picker;
pub mod project_status;
pub mod quickadd;
pub mod search;
pub mod share_token;
pub mod slug;
pub mod tag;
pub mod task;
pub mod template;
pub mod version;
pub mod webhook;
pub mod wikilink;

pub use error::EmberTroveError;

#[cfg(test)]
mod wire_tests {
    /// Every `wire_enum!` table must agree with serde, both ways.
    macro_rules! assert_wire {
        ($ty:ty) => {
            for v in <$ty>::ALL {
                assert_eq!(
                    serde_json::to_value(v).unwrap(),
                    v.as_str(),
                    "{} as_str vs serde",
                    stringify!($ty)
                );
                assert_eq!(<$ty>::parse(v.as_str()).as_ref(), Some(v));
                let back: $ty = serde_json::from_value(v.as_str().into()).unwrap();
                assert_eq!(&back, v);
            }
            assert_eq!(<$ty>::parse("definitely-not-a-variant"), None);
        };
    }

    #[test]
    fn every_wire_enum_matches_serde() {
        assert_wire!(crate::node::NodeType);
        assert_wire!(crate::node::NodeStatus);
        assert_wire!(crate::edge::EdgeType);
        assert_wire!(crate::task::TaskStatus);
        assert_wire!(crate::task::TaskPriority);
        assert_wire!(crate::task::RecurrenceRule);
        assert_wire!(crate::permission::PermissionRole);
    }
}

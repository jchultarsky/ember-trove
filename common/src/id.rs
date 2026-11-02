use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

macro_rules! uuid_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            #[must_use]
            pub fn inner(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }
    };
}

uuid_newtype!(NodeId);
uuid_newtype!(EdgeId);
uuid_newtype!(TagId);
uuid_newtype!(AttachmentId);
uuid_newtype!(PermissionId);
uuid_newtype!(TaskId);
uuid_newtype!(NoteId);
uuid_newtype!(CaptureId);
uuid_newtype!(FavoriteId);
uuid_newtype!(ActivityId);
uuid_newtype!(NodeVersionId);
uuid_newtype!(TemplateId);
uuid_newtype!(SearchPresetId);
uuid_newtype!(NodeLinkId);
uuid_newtype!(WebhookId);
uuid_newtype!(PersonId);
uuid_newtype!(MembershipId);

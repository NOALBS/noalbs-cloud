use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ConnectCommandAction {
    Start,
    Stop,
    Restart,
    UploadLog,
    ProgramStart { id: Uuid },
    ProgramStop { id: Uuid },
    ProgramAdd { program: Program },
    ProgramEdit { program: Program },
    ProgramRemove { id: Uuid },
}

impl ConnectCommandAction {
    pub fn is_obs_control(&self) -> bool {
        matches!(self, Self::Start | Self::Stop | Self::Restart)
    }

    pub fn is_log_upload(&self) -> bool {
        matches!(self, Self::UploadLog)
    }

    pub fn is_program(&self) -> bool {
        matches!(
            self,
            Self::ProgramStart { .. }
                | Self::ProgramStop { .. }
                | Self::ProgramAdd { .. }
                | Self::ProgramEdit { .. }
                | Self::ProgramRemove { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Program {
    pub id: Uuid,
    pub name: String,
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub path: PathBuf,
    pub args: Vec<String>,
    pub start_with_obs: bool,
    pub stop_with_obs: bool,
    pub added_from_dashboard_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProgramStatus {
    pub program: Program,
    pub running: bool,
}

#[cfg(test)]
mod tests {
    use super::{ConnectCommandAction, Program};
    use std::path::PathBuf;
    use uuid::Uuid;

    #[test]
    fn add_and_edit_have_distinct_wire_actions() {
        let program = Program {
            id: Uuid::nil(),
            name: "Streamer.bot".into(),
            path: PathBuf::from("/opt/Streamer.bot"),
            args: Vec::new(),
            start_with_obs: false,
            stop_with_obs: false,
            added_from_dashboard_by: None,
        };
        let add = ConnectCommandAction::ProgramAdd {
            program: program.clone(),
        };
        let edit = ConnectCommandAction::ProgramEdit { program };

        assert!(add.is_program());
        assert!(edit.is_program());
        assert_eq!(serde_json::to_value(add).unwrap()["action"], "program_add");
        assert_eq!(
            serde_json::to_value(edit).unwrap()["action"],
            "program_edit"
        );
    }
}

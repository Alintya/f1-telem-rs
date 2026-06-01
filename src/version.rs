/// Supported game versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameVersion {
    F1_23,
    F1_24,
    F1_25,
}

impl GameVersion {
    /// Resolve from the `packet_format` field in the header.
    pub fn from_format(format: u16) -> Option<Self> {
        match format {
            2023 => Some(Self::F1_23),
            2024 => Some(Self::F1_24),
            2025 => Some(Self::F1_25),
            _ => None,
        }
    }

    /// The `packet_format` value for this version.
    pub fn format(self) -> u16 {
        match self {
            Self::F1_23 => 2023,
            Self::F1_24 => 2024,
            Self::F1_25 => 2025,
        }
    }

    /// The `game_year` byte value (last two digits).
    pub fn year(self) -> u8 {
        match self {
            Self::F1_23 => 23,
            Self::F1_24 => 24,
            Self::F1_25 => 25,
        }
    }
}

impl std::fmt::Display for GameVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::F1_23 => write!(f, "F1 23"),
            Self::F1_24 => write!(f, "F1 24"),
            Self::F1_25 => write!(f, "F1 25"),
        }
    }
}

/// Packet kinds shared across all supported versions.
/// Variants present only in later years still exist here; the dispatch
/// layer returns `UnknownPacketId` when a kind is used with a version
/// that does not support it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PacketKind {
    Motion,
    Session,
    LapData,
    Event,
    Participants,
    CarSetups,
    CarTelemetry,
    CarStatus,
    FinalClassification,
    LobbyInfo,
    CarDamage,
    SessionHistory,
    TyreSets,
    MotionEx,
    /// F1 24+ only.
    TimeTrial,
    /// F1 25+ only.
    LapPositions,
}

impl PacketKind {
    /// Resolve from a raw packet id byte.
    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::Motion),
            1 => Some(Self::Session),
            2 => Some(Self::LapData),
            3 => Some(Self::Event),
            4 => Some(Self::Participants),
            5 => Some(Self::CarSetups),
            6 => Some(Self::CarTelemetry),
            7 => Some(Self::CarStatus),
            8 => Some(Self::FinalClassification),
            9 => Some(Self::LobbyInfo),
            10 => Some(Self::CarDamage),
            11 => Some(Self::SessionHistory),
            12 => Some(Self::TyreSets),
            13 => Some(Self::MotionEx),
            14 => Some(Self::TimeTrial),
            15 => Some(Self::LapPositions),
            _ => None,
        }
    }

    /// The raw packet id for this kind.
    pub fn id(self) -> u8 {
        match self {
            Self::Motion => 0,
            Self::Session => 1,
            Self::LapData => 2,
            Self::Event => 3,
            Self::Participants => 4,
            Self::CarSetups => 5,
            Self::CarTelemetry => 6,
            Self::CarStatus => 7,
            Self::FinalClassification => 8,
            Self::LobbyInfo => 9,
            Self::CarDamage => 10,
            Self::SessionHistory => 11,
            Self::TyreSets => 12,
            Self::MotionEx => 13,
            Self::TimeTrial => 14,
            Self::LapPositions => 15,
        }
    }
}

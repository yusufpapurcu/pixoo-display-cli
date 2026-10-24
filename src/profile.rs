//! Profiles express protocol evidence, not universal device compatibility.
use crate::protocol::ClassicDialect;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceFamily {
    PixooMax,
    Matrix16,
    LargeCanvas64,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    DisplayStatus,
    Native16,
    Native32,
    Native64,
    NativeText,
    IndexedStorage,
    Big64Storage,
    StoredTabs,
    Audio,
    SdCard,
    Alarms,
    Sleep,
    Tools,
    Games,
    Drawing,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodingSupport {
    Typed,
    RequiresEncodedAsset,
    Unresolved,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceEvidence {
    ReadOnlyVerified,
    InheritedMaxObservation,
    GoldenFixtures,
    ModelDependentUnverified,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capability {
    pub encoding: EncodingSupport,
    pub device: DeviceEvidence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageCompletion {
    TerminatorWritten,
    B1Acknowledgment,
    CustomModeStatus,
    TransferOpcode,
}

#[derive(Clone, Copy, Debug)]
pub struct DeviceProfile {
    family: DeviceFamily,
    dialect: ClassicDialect,
    pacing: Duration,
    indexed_completion: StorageCompletion,
}
impl DeviceProfile {
    pub fn pixoo_max() -> Self {
        Self {
            family: DeviceFamily::PixooMax,
            dialect: ClassicDialect::PixooMax,
            pacing: Duration::from_millis(20),
            indexed_completion: StorageCompletion::CustomModeStatus,
        }
    }
    pub fn matrix16() -> Self {
        Self {
            family: DeviceFamily::Matrix16,
            dialect: ClassicDialect::Unescaped,
            pacing: Duration::from_millis(20),
            indexed_completion: StorageCompletion::TerminatorWritten,
        }
    }
    pub fn documented() -> Self {
        Self {
            family: DeviceFamily::Custom,
            dialect: ClassicDialect::Escaped,
            pacing: Duration::from_millis(20),
            indexed_completion: StorageCompletion::TerminatorWritten,
        }
    }
    pub fn large_canvas64() -> Self {
        Self {
            family: DeviceFamily::LargeCanvas64,
            dialect: ClassicDialect::Unescaped,
            pacing: Duration::from_millis(20),
            indexed_completion: StorageCompletion::TerminatorWritten,
        }
    }
    pub fn custom(
        dialect: ClassicDialect,
        pacing: Duration,
        indexed_completion: StorageCompletion,
    ) -> Self {
        Self {
            family: DeviceFamily::Custom,
            dialect,
            pacing,
            indexed_completion,
        }
    }
    pub fn family(self) -> DeviceFamily {
        self.family
    }
    pub fn dialect(self) -> ClassicDialect {
        self.dialect
    }
    pub fn pacing(self) -> Duration {
        self.pacing
    }
    pub fn indexed_completion(self) -> StorageCompletion {
        self.indexed_completion
    }
    /// Evidence describes this implementation and its inherited fixtures, not firmware detection.
    pub fn capability(self, feature: Feature) -> Capability {
        let encoding = match feature {
            Feature::Native64 => EncodingSupport::Unresolved,
            Feature::NativeText | Feature::Big64Storage | Feature::Drawing => {
                EncodingSupport::RequiresEncodedAsset
            }
            _ => EncodingSupport::Typed,
        };
        let device = match (self.family, feature) {
            (DeviceFamily::PixooMax, Feature::DisplayStatus) => DeviceEvidence::ReadOnlyVerified,
            (
                DeviceFamily::PixooMax,
                Feature::Native32 | Feature::IndexedStorage | Feature::StoredTabs,
            ) => DeviceEvidence::InheritedMaxObservation,
            (_, Feature::Native16 | Feature::Native32) => DeviceEvidence::GoldenFixtures,
            _ => DeviceEvidence::ModelDependentUnverified,
        };
        Capability { encoding, device }
    }
}

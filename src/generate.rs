use bevy::prelude::Resource;
pub use world_generation::*;
use std::ops::Deref;
#[derive(Resource)]
pub struct WorldMap(pub world_generation::WorldMap);

impl Deref for WorldMap{
    type Target = world_generation::WorldMap;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl From<&crate::Args> for WorldGenerationParams {
    fn from(value: &crate::Args) -> Self {
        WorldGenerationParams {
            width: value.width,
            height: value.height,
            plate_count: value.plate_count,
            plate_size: value.plate_size,
            continent_count: value.continent_count,
            continent_size: value.continent_size,
            ocean_count: value.ocean_count,
            ocean_size: value.ocean_size,
            scale: 30.0,
        }
    }
}
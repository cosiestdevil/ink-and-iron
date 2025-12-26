use std::collections::HashMap;

use bevy::ecs::resource::Resource;
pub use pathfinding::*;
use petgraph::{Graph, graph::NodeIndex};
use world_generation::CellId;

#[derive(Clone, Resource, Default)]
pub struct PathFinding {
    pub graph: Graph<CellId, f32>,
    pub nodes: HashMap<CellId, NodeIndex>,
}

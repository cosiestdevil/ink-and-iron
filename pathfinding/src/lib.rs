use glam::Vec2;
use glam::Vec3Swizzles;
use petgraph::Graph;
use petgraph::prelude::*;
use std::collections::HashMap;
use world_generation::CellId;
use world_generation::WorldMap;

pub fn get_graph(world_map: &WorldMap) -> (Graph<CellId, f32>, HashMap<CellId, NodeIndex>) {
    let mut graph = Graph::<CellId, f32>::new();
    let mut nodes = HashMap::new();
    for cell in world_map.iter_cells() {
        let cell_id = CellId(cell.site());
        let node = graph.add_node(cell_id);
        nodes.insert(cell_id, node);
    }
    for (cell_id, node) in nodes.iter() {
        let c_pos = world_map.get_position_for_cell(*cell_id);
        for n_cell_id in world_map.get_neighbours(*cell_id) {
            let n_pos = world_map.get_position_for_cell(n_cell_id);
            let length = c_pos.distance(n_pos);
            let height = (c_pos.y - n_pos.y).abs();
            let slope = height / length;
            if slope < 0.3 {
                graph.add_edge(
                    *node,
                    *nodes.get(&n_cell_id).unwrap(),
                    c_pos.distance(n_pos),
                );
            }
        }
    }
    (graph, nodes)
}
#[derive(Clone)]
struct AStarNode {
    cell_id: CellId,
    g: f32,
    h: f32,
    parent: Option<Box<AStarNode>>,
}

impl AStarNode {
    fn new(cell_id: CellId, g: f32, h: f32, parent: Option<Box<AStarNode>>) -> Self {
        Self {
            cell_id,
            g,
            h,
            parent,
        }
    }
    fn f(&self) -> f32 {
        self.g + self.h
    }
}

pub fn a_star(
    start: CellId,
    goal: CellId,
    graph: &Graph<CellId, f32>,
    nodes: &HashMap<CellId, NodeIndex>,
    world_map: &WorldMap,
) -> Option<Vec<CellId>> {
    let mut open_list = vec![AStarNode::new(
        start,
        0.0,
        heuristic(
            world_map.get_position_for_cell(start).xz(),
            world_map.get_position_for_cell(goal).xz(),
        ),
        None,
    )];
    let mut closed_list = Vec::<AStarNode>::new();
    while !open_list.is_empty() {
        let current = open_list
            .iter()
            .min_by(|a, b| a.f().total_cmp(&b.f()))
            .unwrap()
            .clone();
        if current.cell_id == goal {
            return Some(reconstruct_path(current));
        }
        open_list.retain(|n| n.cell_id != current.cell_id);
        closed_list.push(current.clone());
        //let v_cell = voronoi.cell(current.cell_id.0);
        for n_cell_id in world_map.get_neighbours(current.cell_id) {
            if closed_list.iter().any(|n| n.cell_id == n_cell_id) {
                continue;
            }
            let edges = graph
                .edges_connecting(
                    *nodes.get(&current.cell_id).unwrap(),
                    *nodes.get(&n_cell_id).unwrap(),
                )
                .collect::<Vec<_>>();
            if let Some(distance) = edges.first() {
                let tent_g = current.g + distance.weight();
                if let Some(neighbor) = open_list.iter().find(|n| n.cell_id == n_cell_id) {
                    if tent_g >= neighbor.g {
                        continue;
                    }
                } else {
                    open_list.push(AStarNode::new(
                        n_cell_id,
                        tent_g,
                        heuristic(
                            world_map.get_position_for_cell(n_cell_id).xz(),
                            world_map.get_position_for_cell(goal).xz(),
                        ),
                        Some(Box::new(current.clone())),
                    ));
                }
            }
        }
    }
    None
}
fn reconstruct_path(current: AStarNode) -> Vec<CellId> {
    let mut path = Vec::new();
    let mut current = Some(Box::new(current));
    while let Some(node) = current {
        path.push(node.cell_id);
        current = node.parent;
    }
    //path.reverse();
    path
}

fn heuristic(start: Vec2, goal: Vec2) -> f32 {
    start.distance(goal)
}

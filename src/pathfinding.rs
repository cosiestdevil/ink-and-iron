use crate::generate::CellId;
use glam::Vec2;
use glam::Vec3;
use petgraph::Graph;
use petgraph::prelude::*;
use std::collections::HashMap;
use voronoice::Voronoi;

pub fn get_graph(
    voronoi: Voronoi,
    heights: HashMap<CellId, f32>,
) -> (Graph<CellId, f32>, HashMap<CellId, NodeIndex>) {
    let mut graph = Graph::<CellId, f32>::new();
    let mut nodes = HashMap::new();
    for cell in voronoi.iter_cells() {
        let cell_id = CellId(cell.site());
        let node = graph.add_node(cell_id);
        nodes.insert(cell_id, node);
    }
    for (cell_id, node) in nodes.iter() {
        let cell = voronoi.cell(cell_id.0);
        let c_height = heights.get(cell_id).unwrap();
        let c_pos = cell.site_position().to_vec2();
        for n_cell_id in cell.iter_neighbors() {
            let n_cell = voronoi.cell(n_cell_id);
            let n_height = heights.get(&CellId(n_cell_id)).unwrap();
            let n_pos = n_cell.site_position().to_vec2();
            let length = c_pos.distance(n_pos);
            let height = (c_height - n_height).abs();
            let slope = height / length;
            if slope < 0.3 {
                graph.add_edge(
                    *node,
                    *nodes.get(&CellId(n_cell_id)).unwrap(),
                    c_pos.extend(*c_height).distance(n_pos.extend(*n_height)),
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
    f: f32,
    parent: Option<Box<AStarNode>>,
}

impl AStarNode {
    fn new(cell_id: CellId, g: f32, h: f32, parent: Option<Box<AStarNode>>) -> Self {
        Self {
            cell_id,
            g,
            h,
            f: g + h,
            parent,
        }
    }
}

pub fn a_star(
    start: CellId,
    goal: CellId,
    graph: Graph<CellId, f32>,
    nodes: HashMap<CellId, NodeIndex>,
    voronoi: Voronoi,
) -> Option<Vec<CellId>> {
    let mut open_list = vec![AStarNode::new(
        start,
        0.0,
        heuristic(
            voronoi.cell(start.0).site_position().to_vec2(),
            voronoi.cell(goal.0).site_position().to_vec2(),
        ),
        None,
    )];
    let mut closed_list = Vec::<AStarNode>::new();
    while !open_list.is_empty() {
        let current = open_list
            .iter()
            .min_by(|a, b| a.f.total_cmp(&b.f))
            .unwrap()
            .clone();
        if current.cell_id == goal {
            return Some(reconstruct_path(current));
        }
        open_list.retain(|n| n.cell_id != current.cell_id);
        closed_list.push(current.clone());
        let v_cell = voronoi.cell(current.cell_id.0);
        for n_cell_id in v_cell.iter_neighbors() {
            if closed_list.iter().any(|n| n.cell_id.0 == n_cell_id) {
                continue;
            }
            let edges = graph
                .edges_connecting(
                    *nodes.get(&current.cell_id).unwrap(),
                    *nodes.get(&CellId(n_cell_id)).unwrap(),
                )
                .collect::<Vec<_>>();
            if let Some(distance) = edges.first() {
                let tent_g = current.g + distance.weight();
                if let Some(neighbor) = open_list.iter().find(|n| n.cell_id == CellId(n_cell_id)) {
                    if tent_g >= neighbor.g {
                        continue;
                    }
                } else {
                    open_list.push(AStarNode::new(
                        CellId(n_cell_id),
                        tent_g,
                        heuristic(
                            voronoi.cell(n_cell_id).site_position().to_vec2(),
                            voronoi.cell(goal.0).site_position().to_vec2(),
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

pub trait ToVec2 {
    fn to_vec2(&self) -> Vec2;
    fn to_vec3(&self, z: f32) -> Vec3;
}

impl ToVec2 for voronoice::Point {
    fn to_vec2(&self) -> Vec2 {
        Vec2::new(self.x as f32, self.y as f32)
    }

    fn to_vec3(&self, z: f32) -> Vec3 {
        Vec3::new(self.x as f32, self.y as f32, z)
    }
}

fn heuristic(start: Vec2, goal: Vec2) -> f32 {
    start.distance(goal)
}

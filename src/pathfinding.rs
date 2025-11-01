use crate::generate::CellId;
use bevy::platform::collections::HashMap;
use glam::Vec2;
use petgraph::Graph;
use petgraph::prelude::*;
use voronoice::Voronoi;

pub fn get_graph(voronoi: Voronoi) -> (Graph<CellId, f32>,HashMap<CellId,NodeIndex>) {
    let mut graph = Graph::<CellId, f32>::new();
    let mut nodes = HashMap::new();
    for cell in voronoi.iter_cells() {
        let cell_id = CellId(cell.site());
        let node = graph.add_node(cell_id);
        nodes.insert(cell_id, node);
    }
    for (cell_id, node) in nodes.iter() {
        let cell = voronoi.cell(cell_id.0);
        for n_cell_id in cell.iter_neighbors() {
            let n_cell = voronoi.cell(n_cell_id);
            let weight = Vec2::new(cell.site_position().x as f32, cell.site_position().y as f32)
                .distance(Vec2::new(
                    n_cell.site_position().x as f32,
                    n_cell.site_position().y as f32,
                ));
            graph.add_edge(*node, *nodes.get(&CellId(n_cell_id)).unwrap(), weight);
        }
    }
    (graph,nodes)
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
)->Option<Vec<CellId>> {
    let mut open_list = vec![AStarNode::new(
        start,
        0.0,
        heuristic(
            voronoi.cell(start.0).site_position().toVec2(),
            voronoi.cell(goal.0).site_position().toVec2(),
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
            let distance = edges.first().unwrap();
            let tent_g = current.g + distance.weight();
            if let Some(neighbor) = open_list.iter().find(|n| n.cell_id == CellId(n_cell_id)) {
                if tent_g >= neighbor.g{
                    continue;
                }
            } else {
                open_list.push(AStarNode::new(
                    CellId(n_cell_id),
                    tent_g,
                    heuristic(
                        voronoi.cell(n_cell_id).site_position().toVec2(),
                        voronoi.cell(goal.0).site_position().toVec2(),
                    ),
                    Some(Box::new(current.clone())),
                ));
            }
        }
    }
    None
}
fn reconstruct_path(current:AStarNode)->Vec<CellId>{
    let mut path = Vec::new();
    let mut current = Some(Box::new(current));
    while let Some(node) = current{
        path.push(node.cell_id);
        current = node.parent;
    }
    path.reverse();
    path
}

trait ToVec2 {
    fn toVec2(&self) -> Vec2;
}

impl ToVec2 for voronoice::Point {
    fn toVec2(&self) -> Vec2 {
        Vec2::new(self.x as f32, self.y as f32)
    }
}

fn heuristic(start: Vec2, goal: Vec2) -> f32 {
    start.distance(goal)
}

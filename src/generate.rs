use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    hash::Hash,
    ops::Deref
};

use bevy::prelude::*;
use geo::{Contains, CoordsIter, Polygon, unary_union};
use glam::Vec2;
use noise::{Fbm, NoiseFn, Perlin, RidgedMulti};
use rand::{
    Rng,
    distr::{Distribution, Uniform},
};
use voronoice::*;

use crate::pathfinding::ToVec2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellId(pub usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlateId(usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContinentId(usize);
#[derive(Resource)]
pub struct WorldMap {
    pub scale: f32,
    pub voronoi: Voronoi,
    pub cell_height: HashMap<CellId, f32>,
    polygons: HashMap<CellId, geo::Polygon>,
}
impl WorldMap {
    pub fn get_cell_for_position(&self, pos: Vec2) -> Option<CellId> {
        for (cell_id, poly) in self.polygons.iter() {
            if poly.contains(&geo::point!(x:(pos.x/self.scale) as f64,y:(pos.y/self.scale) as f64)) {
                return Some(*cell_id);
            }
        }
        None
    }
    pub fn get_position_for_cell(&self, id:CellId)->Vec2{
        let cell = self.voronoi.cell(id.0);
        cell.site_position().to_vec2() * self.scale
    }
}
impl Deref for CellId {
    type Target = usize;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Deref for PlateId {
    type Target = usize;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Deref for ContinentId {
    type Target = usize;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl From<ContinentId> for usize {
    fn from(val: ContinentId) -> Self {
        val.0
    }
}

#[derive(Clone, Copy)]
pub struct WorldGenerationParams {
    width: f64,
    height: f64,
    plate_count: usize,
    plate_size: usize,
    continent_count: usize,
    continent_size: usize,
    ocean_count: usize,
    ocean_size: usize,
    scale:f32
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
            scale:30.0,
        }
    }
}

pub fn generate_world<R: Rng + Clone>(
    params: WorldGenerationParams,
    mut rng: &mut R,
) -> anyhow::Result<WorldMap> {
    let WorldGenerationParams {
        width,
        height,
        plate_count,
        plate_size,
        continent_count,
        continent_size,
        ocean_count,
        ocean_size,
        scale
    } = params;
    let fbm = Fbm::<Perlin>::new(rng.next_u32());
    let ridged_multi = RidgedMulti::<Perlin>::new(rng.next_u32());
    let my_voronoi = generate(&mut rng, width, height, plate_count * plate_size)?;

    let mut plates: HashMap<CellId, PlateId> = HashMap::new();

    rng.sample_iter(rand::distr::Uniform::new(0, plate_count * plate_size).unwrap())
        .take(plate_count)
        .enumerate()
        .for_each(|(i, v)| {
            plates.insert(CellId(v), PlateId(i));
        });
    while plates.len() < my_voronoi.sites().len() {
        let current_plates = plates.clone();
        for (k, v) in current_plates.iter() {
            let cell = my_voronoi.cell(**k);
            set_neighbour_plate(&cell, &mut plates, *v);
            if plates.len() >= my_voronoi.sites().len() {
                break;
            }
        }
    }
    let plates_to_cells: HashMap<PlateId, Vec<CellId>> = crate::helpers::invert_borrowed(&plates);
    let mut hull_plates: HashMap<PlateId, geo::Polygon> = HashMap::new();
    for plate in plates_to_cells.keys() {
        let mut polygons: Vec<Polygon> = vec![];
        for cell_id in plates_to_cells.get(plate).unwrap() {
            let cell = my_voronoi.cell(**cell_id);
            let poly = geo::Polygon::new(
                geo::LineString::from(
                    cell.iter_vertices()
                        .map(|p| geo::Coord { x: p.x, y: p.y })
                        .collect::<Vec<_>>(),
                ),
                vec![],
            );
            polygons.push(poly);
        }
        let multi_polygon = unary_union(polygons.iter());
        let hull = Polygon::new(
            geo::LineString::from(multi_polygon.exterior_coords_iter().collect::<Vec<_>>()),
            vec![],
        );
        hull_plates.insert(*plate, hull);
    }

    let continents_voronoi = generate(
        &mut rng,
        width,
        height,
        continent_count * continent_size + ocean_count * ocean_size,
    )?;
    let mut continents: HashMap<CellId, ContinentId> = HashMap::new();
    rng.sample_iter(rand::distr::Uniform::new(0, continent_count * continent_size).unwrap())
        .take(continent_count)
        .enumerate()
        .for_each(|(i, v)| {
            continents.insert(CellId(v), ContinentId(i));
        });
    while continents.len() < continents_voronoi.sites().len() - (ocean_count * ocean_size) {
        let current_plates = continents.clone();
        for (k, v) in current_plates.iter() {
            let cell = continents_voronoi.cell(**k);
            set_neighbour_continent(&mut rng, &cell, &mut continents, *v, &hull_plates);
            if continents.len() >= continents_voronoi.sites().len() {
                break;
            }
        }
    }
    let last_continent_id = continents.values().copied().max().unwrap_or(ContinentId(0));
    let mut island_index = 0;
    for cell in continents_voronoi.iter_cells() {
        let cell_id = CellId(cell.site());
        if !continents.contains_key(&cell_id) && !cell.is_on_hull() {
            if rng.random_bool(0.01) {
                let continent_id = ContinentId(last_continent_id.0 + island_index);
                island_index += 1;
                continents.insert(CellId(cell.site()), continent_id);
            }
        } else if cell.is_on_hull() {
            let continent = *continents.get(&cell_id).unwrap_or(&ContinentId(0));
            continents.retain(|_k, v| *v != continent);
        }
    }
    let continent_ids = continents
        .values()
        .copied()
        .collect::<HashSet<ContinentId>>()
        .iter()
        .copied()
        .collect::<Vec<_>>();
    for continent in continent_ids {
        let cells = continents
            .iter()
            .filter(|(_k, v)| **v == continent)
            .map(|(k, _v)| *k)
            .collect::<Vec<CellId>>();
        for cell_id in cells {
            if !continents.contains_key(&cell_id) {
                continue;
            }
            let cell = continents_voronoi.cell(*cell_id);
            for n in cell.iter_neighbors() {
                let n_id = CellId(n);
                if let Some(neighbor_continent) = continents.get(&n_id) {
                    let neighbor_continent = *neighbor_continent;
                    if neighbor_continent != continent {
                        let neighbor_continent_cells = continents
                            .iter()
                            .filter(|(_k, v)| **v == neighbor_continent)
                            .map(|(k, _v)| *k)
                            .collect::<Vec<CellId>>();
                        for cn in neighbor_continent_cells {
                            continents.insert(cn, continent);
                        }
                    }
                }
            }
        }
    }

    let neighbours = build_neighbors_from_voronoi(&continents_voronoi);
    let mut cells: Vec<Cell> = Vec::new();
    let mut plate_to_cells: HashMap<PlateId, Vec<CellId>> = HashMap::new();
    
    let mut cell_polys = HashMap::new();
    for v_cell in continents_voronoi.iter_cells() {
        let cell_id = CellId(v_cell.site());
        let plate = if let Some(p) = hull_plates.iter().find(|(_k, v)| {
            v.contains(&geo::point!(x: v_cell.site_position().x, y: v_cell.site_position().y))
        }) {
            *p.0
        } else {
            PlateId(usize::MAX)
        };
        let cell = Cell {
            id: cell_id,
            pos: glam::Vec2 {
                x: v_cell.site_position().x as f32,
                y: v_cell.site_position().y as f32,
            },
            continent: if v_cell.is_on_hull() {
                None
            } else {
                continents.get(&cell_id).cloned()
            },
            is_ocean: if v_cell.is_on_hull() {
                true
            } else {
                !continents.contains_key(&cell_id)
            },
            neighbors: neighbours[&cell_id].clone(),
            plate,
        };
        if let Some(p_cells) = plate_to_cells.get_mut(&cell.plate) {
            p_cells.push(cell_id);
        } else {
            plate_to_cells.insert(cell.plate, vec![cell_id]);
        }
        cells.push(cell);
        let poly = geo::Polygon::new(
            geo::LineString::from(
                v_cell
                    .iter_vertices()
                    .map(|p| geo::Coord { x: p.x, y: p.y })
                    .collect::<Vec<_>>(),
            ),
            vec![],
        );
        cell_polys.insert(cell_id, poly);
    }
    println!("plates: {:?}",plate_to_cells.keys());
    
    let mut plates = HashMap::new();
    for (plateid, cells) in plate_to_cells {
        let crust = if most_common_bool(cells.iter().map(|c| continents.contains_key(c))) {
            Crust::Continental
        } else {
            Crust::Oceanic
        };
        let plate = Plate {
            id: plateid,
            crust,
            vel: rand::distr::Uniform::new(Vec2::new(-1.0, -1.0), Vec2::new(1.0, 1.0))
                .unwrap()
                .sample(&mut rng),
            age_myr: rng.sample(Uniform::new(120.0, 3000.0).unwrap()),
            buoyancy: 0.0,
        };
        plates.insert(plateid,plate);
    }

    let noise_scale = 50.0;
    let mut h = generate_heightmap(&cells, plates, |p| {
        // Simple FBM + ridged noise
        let a = fbm.get([p.x as f64 * noise_scale, p.y as f64 * noise_scale]) as f32 * 0.5;
        let b = ridged_multi.get([p.x as f64 * noise_scale, p.y as f64 * noise_scale]) as f32 * 1.0;
        (a, b)
    });
    println!(
        "heightmap max: {}, min: {}",
        h.iter().cloned().fold(f32::MIN, f32::max),
        h.iter().cloned().fold(f32::MAX, f32::min)
    );
    normalize_split01_in_place(h.as_mut_slice());
    println!(
        "heightmap max: {}, min: {}",
        h.iter().cloned().fold(f32::MIN, f32::max),
        h.iter().cloned().fold(f32::MAX, f32::min)
    );
    let cells_height = cells
        .iter()
        .enumerate()
        .map(|(i, c)| (c.id, h[i]))
        .collect::<HashMap<CellId, f32>>();
    // svg::render(
    //     width,
    //     height,
    //     &continents_voronoi,
    //     &continents,
    //     cells_height,
    //     "continents_voronoi_merged.svg",
    // )?;
    Ok(WorldMap {
        scale,
        voronoi: continents_voronoi,
        cell_height: cells_height,
        polygons: cell_polys,
    })
}
pub fn normalize_split01_in_place(v: &mut [f32]) -> Option<(f32, f32)> {
    // 1) Scan finite min/max
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    let mut saw_finite = false;

    for &x in v.iter() {
        if x.is_finite() {
            saw_finite = true;
            if x < min {
                min = x;
            }
            if x > max {
                max = x;
            }
        }
    }
    if !saw_finite {
        return None; // all values were NaN/∞
    }

    // Precompute inverses (only used if the side exists)
    let inv_neg = if min < 0.0 { 1.0 / (-min) } else { 0.0 };
    let inv_pos = if max > 0.0 { 1.0 / max } else { 0.0 };

    // 2) Transform
    for x in v.iter_mut() {
        let xi = *x;
        if !xi.is_finite() {
            continue;
        }

        *x = if xi < 0.0 {
            // Map [min..0] -> [0..0.5]
            0.5 * (xi - min) * inv_neg
        } else if xi > 0.0 {
            // Map [0..max] -> [0.5..1]
            0.5 + 0.5 * xi * inv_pos
        } else {
            // Exactly zero
            0.5
        };
    }

    Some((min, max))
}

pub fn most_common_bool<I>(iter: I) -> bool
where
    I: IntoIterator<Item = bool>,
{
    let diff = iter
        .into_iter()
        .fold(0isize, |acc, b| acc + if b { 1 } else { -1 });
    match diff.cmp(&0) {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => true, // tie (or empty)
    }
}
#[derive(Clone)]
struct Cell {
    id: CellId,
    pos: glam::Vec2, // site/centroid in world units
    neighbors: smallvec::SmallVec<[CellId; 8]>,
    plate: PlateId,
    continent: Option<ContinentId>,
    is_ocean: bool, // from your continent/island grouping
}

#[derive(Clone, Copy)]
enum Crust {
    Continental,
    Oceanic,
}

#[derive(Clone)]
struct Plate {
    id: PlateId,
    vel: glam::Vec2, // world units per Myr (or arbitrary)
    crust: Crust,    // dominant crust type
    age_myr: f32,    // optional (oceanic deepens with age)
    buoyancy: f32,   // 0..1 (optional); else derive from crust
}

// For each adjacency edge crossing a plate boundary:
#[derive(Clone, Copy)]
enum BoundaryType {
    Convergent,
    Divergent,
    Transform,
}

struct BoundaryEdge {
    a: CellId,
    b: CellId, // adjacent map cells with plate[a] != plate[b]
    bt: BoundaryType,
    n_ab: glam::Vec2, // unit normal pointing from b -> a
    ocean_on_a: bool, // is the a-side ocean (for subduction side tests)
}

struct Fields {
    d_coast: Vec<f32>, // + on land, - in ocean (cell-center metric)
    d_conv: Vec<f32>,
    d_div: Vec<f32>,
    d_tr: Vec<f32>,
    d_conv_ocean_side: Vec<f32>, // for trenches
    d_conv_land_side: Vec<f32>,  // for arcs
}

fn build_neighbors_from_voronoi(
    voronoi: &Voronoi,
) -> HashMap<CellId, smallvec::SmallVec<[CellId; 8]>> {
    let mut res = HashMap::new();
    for cell in voronoi.iter_cells() {
        let cell_id = CellId(cell.site());
        let mut neighbors = smallvec::SmallVec::<[CellId; 8]>::new();
        for n in cell.iter_neighbors() {
            neighbors.push(CellId(n));
        }
        res.insert(cell_id, neighbors);
    }
    res
    // From voronoice you can iterate half-edges per cell and collect neighboring cell indices.
    // Omitted here—use diagram.cell(i).iter_edges() to find adjacent cell ids.
    // unimplemented!()
}

fn shoreline_cells(cells: &[Cell]) -> Vec<CellId> {
    cells
        .iter()
        .filter(|c| {
            c.neighbors
                .iter()
                .any(|&n| cells[n.0].is_ocean != c.is_ocean)
        })
        .map(|c| c.id)
        .collect()
}

fn signed_coast_distance(cells: &[Cell]) -> Vec<f32> {
    // Multi-source BFS in graph steps, then convert to metric by multiplying by avg edge length.
    let n = cells.len();
    let mut dist = vec![i32::MAX; n];
    let mut q = std::collections::VecDeque::new();

    for &s in &shoreline_cells(cells) {
        dist[s.0] = 0;
        q.push_back(s);
    }
    while let Some(u) = q.pop_front() {
        let du = dist[u.0];
        for &v in &cells[u.0].neighbors {
            if dist[v.0] == i32::MAX {
                dist[v.0] = du + 1;
                q.push_back(v);
            }
        }
    }

    // Convert to signed f32. Scale step length to world units (avg neighbor distance).
    let mean_edge = 0.5
        * cells
            .iter()
            .flat_map(|c| {
                c.neighbors
                    .iter()
                    .map(move |&n| (cells[*n].pos - c.pos).length())
            })
            .sum::<f32>()
        / cells
            .iter()
            .map(|c| c.neighbors.len() as f32)
            .sum::<f32>()
            .max(1.0);

    dist.iter()
        .enumerate()
        .map(|(i, d)| {
            let s = if cells[i].is_ocean { -1.0 } else { 1.0 };
            s * (*d as f32) * mean_edge
        })
        .collect()
}
fn classify_boundaries(cells: &[Cell], plates: &HashMap<PlateId,Plate>) -> Vec<BoundaryEdge> {
    let mut edges = Vec::<BoundaryEdge>::new();
    let mut seen = std::collections::HashSet::<(CellId, CellId)>::new();

    for c in cells {
        for &n in &c.neighbors {
            let (a, b) = (c.id, n);
            if a.0 < b.0 && cells[a.0].plate != cells[b.0].plate && seen.insert((a, b)) {
                let pa = plates.get(&cells[a.0].plate).unwrap();
                let pb = plates.get(&cells[b.0].plate).unwrap();
                let v_rel = pa.vel - pb.vel;
                let n_ab = (cells[a.0].pos - cells[b.0].pos).normalize(); // b->a

                let s = v_rel.dot(n_ab); // approach (>0) vs separate (<0)
                //let tmag = (v_rel.x*v_rel.x + v_rel.y*v_rel.y - s*s).sqrt(); // tangential
                let amag = v_rel.length();

                // Heuristics: mostly normal motion => convergent/divergent, else transform
                let bt = if s.abs() >= 0.6 * amag {
                    if s > 0.0 {
                        BoundaryType::Convergent
                    } else {
                        BoundaryType::Divergent
                    }
                } else {
                    BoundaryType::Transform
                };

                edges.push(BoundaryEdge {
                    a,
                    b,
                    bt,
                    n_ab,
                    ocean_on_a: cells[a.0].is_ocean,
                });
            }
        }
    }
    edges
}
struct BoundaryDistances {
    conv: Vec<f32>,
    div: Vec<f32>,
    tr: Vec<f32>,
    conv_ocean: Vec<f32>,
    conv_land: Vec<f32>,
}
fn distance_to_boundary(cells: &[Cell], edges: &[BoundaryEdge]) -> BoundaryDistances {
    // Seed sets for BFS by boundary type
    use BoundaryType::*;
    let n = cells.len();
    let mean_edge = 0.5
        * cells
            .iter()
            .flat_map(|c| {
                c.neighbors
                    .iter()
                    .map(move |&n| (cells[*n].pos - c.pos).length())
            })
            .sum::<f32>()
        / cells
            .iter()
            .map(|c| c.neighbors.len() as f32)
            .sum::<f32>()
            .max(1.0);

    let mut conv = vec![i32::MAX; n];
    let mut div = vec![i32::MAX; n];
    let mut tr = vec![i32::MAX; n];
    let mut conv_ocean = vec![i32::MAX; n]; // distance measured staying on ocean side
    let mut conv_land = vec![i32::MAX; n];

    // Seed all cells that *touch* a boundary edge of a given type
    let mut seed_conv = Vec::new();
    let mut seed_div = Vec::new();
    let mut seed_tr = Vec::new();
    let mut seed_conv_ocean = Vec::new();
    let mut seed_conv_land = Vec::new();

    for e in edges {
        match e.bt {
            Convergent => {
                seed_conv.push(e.a);
                seed_conv.push(e.b);
                if cells[e.a.0].is_ocean {
                    seed_conv_ocean.push(e.a);
                }
                if cells[e.b.0].is_ocean {
                    seed_conv_ocean.push(e.b);
                }
                if !cells[e.a.0].is_ocean {
                    seed_conv_land.push(e.a);
                }
                if !cells[e.b.0].is_ocean {
                    seed_conv_land.push(e.b);
                }
            }
            Divergent => {
                seed_div.push(e.a);
                seed_div.push(e.b);
            }
            Transform => {
                seed_tr.push(e.a);
                seed_tr.push(e.b);
            }
        }
    }

    fn multi_bfs(cells: &[Cell], out: &mut [i32], seeds: &[CellId]) {
        let mut q = std::collections::VecDeque::new();
        for &s in seeds {
            if out[s.0] > 0 {
                out[s.0] = 0;
                q.push_back(s);
            }
        }
        while let Some(u) = q.pop_front() {
            let du = out[u.0];
            for &v in &cells[u.0].neighbors {
                if out[v.0] == i32::MAX {
                    out[v.0] = du + 1;
                    q.push_back(v);
                }
            }
        }
    }

    multi_bfs(cells, &mut conv, &seed_conv);
    multi_bfs(cells, &mut div, &seed_div);
    multi_bfs(cells, &mut tr, &seed_tr);
    multi_bfs(cells, &mut conv_ocean, &seed_conv_ocean);
    multi_bfs(cells, &mut conv_land, &seed_conv_land);

    let to_f32 = |v: Vec<i32>| v.into_iter().map(|d| (d as f32) * mean_edge).collect();
    BoundaryDistances {
        conv: to_f32(conv),
        div: to_f32(div),
        tr: to_f32(tr),
        conv_ocean: to_f32(conv_ocean),
        conv_land: to_f32(conv_land),
    }
}
/// Parameters controlling procedural terrain synthesis.
///
/// This set of scalar parameters tunes distinct contributions to a generated
/// heightfield: broad baselines (continents/oceans), orogenic and tectonic
/// features (mountains, trenches, arcs, ridges), coastal geometry (shelves and
/// slopes), and small‑scale noise. All values are floating‑point and are
/// interpreted as amplitudes, lengths, spreads, offsets or blend weights
/// according to the parameter name.
/// Baseline elevations and sea level
/// - a_cont: baseline amplitude applied to continental regions.
/// - a_ocean: baseline amplitude/depth applied to ocean basins.
///
/// Mountain, trench and volcanic arc controls
/// - a_mtn: amplitude of mountain ranges (height of peaks).
/// - w_mtn: characteristic horizontal scale/width of mountain belts.
/// - p_mtn: placement/periodicity parameter affecting mountain distribution.
/// - a_trench: amplitude/depth of oceanic trenches.
/// - sigma_tr: spatial spread (standard deviation) of trench features.
/// - a_arc: amplitude of volcanic arc features (elevation or depression).
/// - delta_arc: lateral offset applied to arc placement relative to boundaries.
/// - sigma_arc: spatial spread (standard deviation) of arc features.
///
/// Mid‑ocean ridge and transform fault controls
/// - a_mor: amplitude of mid‑ocean ridge features.
/// - w_mor: characteristic width/scale of ridge crests.
/// - p_mor: periodicity/phase parameter for ridge segmentation.
/// - a_tr: amplitude for transform fault / fracture zone features.
/// - w_tr: width/scale of transform features.
///
/// Coasts, continental plains, shelves and slopes
/// - a_plains: amplitude/height of lowland plains on continents.
/// - l_plains: horizontal extent/characteristic length of plains.
/// - d_shelf: depth of the continental shelf (positive downward).
/// - l_shelf: horizontal length/extent of the shelf region.
/// - d_slope: vertical drop across the continental slope.
/// - l_slope: horizontal length of the slope transition.
/// - d_abyss: depth of the abyssal plain / deep ocean basins.
///
/// Procedural noise and blending
/// - warp: amount of positional warping applied to sampled coordinates (noise warp).
/// - w_fbm: blending weight for FBM (fractional Brownian motion) noise component.
/// - w_ridge: blending weight for ridge-style noise component.
///
/// Notes:
/// - Parameters are typically tuned together: amplitudes set magnitudes,
///   length/width parameters control spatial scales, and sigma/delta values
///   control spreads and offsets. Noise weights modulate detail and roughness.
struct Params {
    // Baseline
    ///baseline amplitude applied to continental regions.
    a_cont: f32,
    ///baseline amplitude/depth applied to ocean basins.
    a_ocean: f32,
    // Mountains/trench/arc
    ///amplitude of mountain ranges (height of peaks).
    a_mtn: f32,
    ///characteristic horizontal scale/width of mountain belts.
    w_mtn: f32,
    ///placement/periodicity parameter affecting mountain distribution.
    p_mtn: f32,
    ///amplitude/depth of oceanic trenches.
    a_trench: f32,
    ///spatial spread (standard deviation) of trench features.
    sigma_tr: f32,
    ///amplitude of volcanic arc features (elevation or depression).
    a_arc: f32,
    ///lateral offset applied to arc placement relative to boundaries.
    delta_arc: f32,
    ///spatial spread (standard deviation) of arc features.
    sigma_arc: f32,
    // MOR & transform
    ///amplitude of mid‑ocean ridge features.
    a_mor: f32,
    ///characteristic width/scale of ridge crests.
    w_mor: f32,
    ///periodicity/phase parameter for ridge segmentation.
    p_mor: f32,
    ///amplitude for transform fault / fracture zone features.
    a_tr: f32,
    ///width/scale of transform features.
    w_tr: f32,
    // Coast & shelves
    ///amplitude/height of lowland plains on continents.
    a_plains: f32,
    ///horizontal extent/characteristic length of plains.
    l_plains: f32,
    ///depth of the continental shelf (positive downward).
    d_shelf: f32,
    ///horizontal length/extent of the shelf region.
    l_shelf: f32,
    ///vertical drop across the continental slope.
    d_slope: f32,
    ///horizontal length of the slope transition.
    l_slope: f32,
    ///depth of the abyssal plain / deep ocean basins.
    d_abyss: f32,
    // Noise
    ///amount of positional warping applied to sampled coordinates (noise warp).
    warp: f32,
    ///blending weight for FBM (fractional Brownian motion) noise component.
    w_fbm: f32,
    ///blending weight for ridge-style noise component.
    w_ridge: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            a_cont: 50.0,
            a_ocean: -50.0,
            a_mtn: 50.0,
            w_mtn: 1.0,
            p_mtn: 50.0,
            a_trench: -4.0,
            sigma_tr: 2.0,
            a_arc: 2.2,
            delta_arc: 1.1,
            sigma_arc: 4.5,
            a_mor: 5.4,
            w_mor: 1.0,
            p_mor: 1.2,
            a_tr: 0.2,
            w_tr: 1.0,
            a_plains: 1.4,
            l_plains: 1.3,
            d_shelf: 10.0,
            l_shelf: 1000.2,
            d_slope: 20.4,
            l_slope: 20.0,
            d_abyss: 0.8,
            warp: 4.0,
            w_fbm: 0.6,
            w_ridge: 1.0,
        }
    }
}
fn laplacian_smooth(cells: &[Cell], h: &mut [f32], iters: usize) {
    let mut tmp = h.to_vec();
    for _ in 0..iters {
        for (i, c) in cells.iter().enumerate() {
            if c.neighbors.is_empty() {
                tmp[i] = h[i];
                continue;
            }
            let s: f32 = c.neighbors.iter().map(|&n| h[n.0]).sum();
            tmp[i] = 0.5 * h[i] + 0.5 * (s / (c.neighbors.len() as f32));
        }
        h.copy_from_slice(&tmp);
    }
}
fn assemble_height(
    cells: &[Cell],
    plates: &HashMap<PlateId,Plate>,
    fields: &Fields,
    noise: &mut impl FnMut(glam::Vec2) -> (f32, f32), // returns (fbm, ridged)
    p: &Params,
) -> Vec<f32> {
    let n = cells.len();
    let mut h = vec![-1.0f32; n];

    // 1) Base crust
    for (i, c) in cells.iter().enumerate() {
        let plate = &plates[&c.plate];
        let base = match plate.crust {
            Crust::Continental => p.a_cont,
            Crust::Oceanic => {
                // Optional age deepening
                p.a_ocean - 0.006 * plate.age_myr.min(120.0)
            }
        };
        h[i] = base;
    }
    for (i, c) in cells.iter().enumerate() {
        let plate = &plates[&c.plate];
        h[i] = if c.is_ocean {
            p.a_ocean - 0.6 * plate.age_myr.min(120.0)
        } else {
            p.a_cont
        }
    }
    laplacian_smooth(cells, &mut h, 2);
    // 2) Boundary features
    for (i, c) in cells.iter().enumerate() {
        // Convergent mountains (C-C or general ridge)
        let u = (1.0 - fields.d_conv[i] / p.w_mtn)
            .clamp(0.0, 1.0)
            .powf(p.p_mtn);
        h[i] += p.a_mtn * u;

        // Subduction asymmetry (O-C): trench on ocean side, volcanic arc inland
        if c.is_ocean {
            let x = fields.d_conv_ocean_side[i];
            h[i] += p.a_trench * (-(x / p.sigma_tr).powi(2)).exp();
        } else {
            let x = fields.d_conv_land_side[i];
            h[i] += p.a_arc * (-((x - p.delta_arc).powi(2)) / (p.sigma_arc * p.sigma_arc)).exp();
        }

        // MOR
        let v = (1.0 - fields.d_div[i] / p.w_mor)
            .clamp(0.0, 1.0)
            .powf(p.p_mor);
        h[i] += p.a_mor * v;

        // Transform
        let t = (1.0 - fields.d_tr[i] / p.w_tr).clamp(0.0, 1.0);
        h[i] += p.a_tr * t;
    }

    // 3) Coast & shelves using signed d_coast
    for (i, _c) in cells.iter().enumerate() {
        let dc = fields.d_coast[i];
        if dc >= 0.0 {
            h[i] += p.a_plains * smoothstep(0.0, p.l_plains, dc);
        } else {
            let x = -dc;
            h[i] += -p.d_shelf * smoothstep(0.0, p.l_shelf, x);
            h[i] += -p.d_slope * smoothstep(p.l_shelf, p.l_shelf + p.l_slope, x);
            h[i] += -p.d_abyss * smoothstep(p.l_shelf + p.l_slope, 1.0e9, x);
        }
    }

    // 4) Warped noise (masked)
    for (i, c) in cells.iter().enumerate() {
        // domain warp
        let (q1, q2) = noise(glam::Vec2 {
            x: c.pos.x + 37.0,
            y: c.pos.y - 19.0,
        });
        let wx = glam::Vec2 {
            x: c.pos.x + p.warp * q1,
            y: c.pos.y + p.warp * q2,
        };
        let (fbm, ridged) = noise(wx);

        let land_mask = if c.is_ocean { 0.4 } else { 1.0 };
        let mtn_mask = (1.0 - fields.d_conv[i] / p.w_mtn).clamp(0.0, 1.0);
        h[i] += p.w_fbm * fbm * land_mask;
        h[i] += p.w_ridge * ridged * (0.2 + 0.8 * mtn_mask);
    }

    h
}

fn carve_rivers(cells: &[Cell], h: &mut [f32], threshold: usize) {
    let n = cells.len();
    // Choose steepest neighbor as downslope pointer
    let mut to = vec![None::<CellId>; n];
    let mut indeg = vec![0usize; n];
    for (i, c) in cells.iter().enumerate() {
        let mut best = None::<(f32, CellId)>;
        for &nb in &c.neighbors {
            let s = h[i] - h[nb.0];
            if s > 0.0 && best.map(|(bs, _)| s > bs).unwrap_or(true) {
                best = Some((s, nb));
            }
        }
        if let Some((_, nb)) = best {
            to[i] = Some(nb);
            indeg[nb.0] += 1;
        }
    }

    // Topological order (forest), accumulate flow
    let mut flow = vec![1.0f32; n];
    let mut q: std::collections::VecDeque<CellId> =
        (0..n).filter(|&i| indeg[i] == 0).map(CellId).collect();

    while let Some(u) = q.pop_front() {
        if let Some(v) = to[u.0] {
            flow[v.0] += flow[u.0];
            indeg[v.0] -= 1;
            if indeg[v.0] == 0 {
                q.push_back(v);
            }
        }
    }

    // Carve: width ~ sqrt(flow), depth ~ flow^β (keep small)
    let a_river = 0.35f32;
    let beta = 0.4f32;
    // let k = 0.9f32;

    for (i, f) in flow.iter().enumerate() {
        if *f as usize >= threshold {
            //let width_steps = (k * f.sqrt()).ceil() as i32;
            for &nb in &cells[i].neighbors {
                // simple 1-ring widening; you can do a BFS ring of width_steps
                let depth = a_river * f.powf(beta);
                // smooth falloff over neighbor ring would look better; simplified here:
                h[i] -= depth * 0.7;
                h[nb.0] -= depth * 0.3;
            }
        }
    }
}
fn generate_heightmap(
    cells: &[Cell],
    plates: HashMap<PlateId,Plate>,
    mut noise: impl FnMut(glam::Vec2) -> (f32, f32),
) -> Vec<f32> {
    // Distances
    let d_coast = signed_coast_distance(cells);
    let edges = classify_boundaries(cells, &plates);
    let BoundaryDistances {
        conv,
        div,
        tr,
        conv_ocean,
        conv_land,
    } = distance_to_boundary(cells, &edges);

    let fields = Fields {
        d_coast,
        d_conv: conv,
        d_div: div,
        d_tr: tr,
        d_conv_ocean_side: conv_ocean,
        d_conv_land_side: conv_land,
    };

    // Height layers
    let p = Params::default();
    let mut h = assemble_height(cells, &plates, &fields, &mut noise, &p);

    // Rivers & erosion
    carve_rivers(
        cells,
        &mut h,
        ((0.005 * cells.len() as f32) as usize).max(3),
    );

    // Normalize sea level to desired ratio (optional if you’ve fixed land/ocean)
    //set_sea_level(&mut h, 0.67, None);

    h
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn set_neighbour_plate(cell: &VoronoiCell, plates: &mut HashMap<CellId, PlateId>, i: PlateId) {
    for n in cell
        .iter_neighbors()
        .filter(|c| !plates.contains_key(&CellId(*c)))
        .collect::<Vec<_>>()
    {
        let n_id = CellId(n);
        plates.entry(n_id).or_insert(i);
    }
}
fn set_neighbour_continent<R: Rng>(
    rng: &mut R,
    cell: &VoronoiCell,
    continents: &mut HashMap<CellId, ContinentId>,
    i: ContinentId,
    hulls: &HashMap<PlateId, geo::Polygon>,
) {
    let current_plate_id = hulls.keys().find(|plate_id| {
        let hull = &hulls[plate_id];
        let site = cell.site_position();
        hull.contains(&geo::Point::new(site.x, site.y))
    });
    if let Some(current_plate_id) = current_plate_id {
        for n in cell
            .iter_neighbors()
            .filter(|c| !continents.contains_key(&CellId(*c)))
            .collect::<Vec<_>>()
        {
            let n_id = CellId(n);
            if !continents.contains_key(&n_id) {
                if let Some(plate_id) = hulls.keys().find(|plate_id| {
                    let hull = &hulls[plate_id];
                    let site = cell.site_position();
                    hull.contains(&geo::Point::new(site.x, site.y))
                }) && plate_id == current_plate_id
                {
                    continents.insert(n_id, i);
                    continue;
                }
                if rng.random_bool(0.3) {
                    continents.insert(n_id, i);
                }
            }
        }
    }
}
fn generate<R: Rng>(
    rng: &mut R,
    width: f64,
    height: f64,
    point_count: usize,
) -> anyhow::Result<Voronoi> {
    let sites: Vec<Point> = (0..point_count)
        .map(|_| Point {
            x: rng.random_range(0.0..width),
            y: rng.random_range(0.0..height),
        })
        .collect();

    let my_voronoi = build_voronoi(sites, width, height)?;
    Ok(my_voronoi)
}
fn build_voronoi(sites: Vec<Point>, width: f64, height: f64) -> anyhow::Result<Voronoi> {
    let my_voronoi = VoronoiBuilder::default()
        .set_sites(sites)
        .set_bounding_box(BoundingBox::new(
            Point {
                x: width / 2.0,
                y: height / 2.0,
            },
            width,
            height,
        ))
        .set_lloyd_relaxation_iterations(5)
        .build()
        .expect("Failed to build Voronoi diagram");
    Ok(my_voronoi)
}

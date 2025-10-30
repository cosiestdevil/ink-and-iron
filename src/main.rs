use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
};

use anyhow::Ok;
use clap::Parser;
use geo::{Contains, CoordsIter, Polygon, unary_union};
use num::{Num};
use rand::{Rng, SeedableRng};

use rand_chacha::ChaCha20Rng;
use voronoice::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CellId(usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PlateId(usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct ContinentId(usize);

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
impl Into<usize> for ContinentId {
    fn into(self) -> usize {
        self.0
    }
}
#[derive(Parser,Debug)]
struct Args{
    #[arg(long, default_value_t = 16.0)]
    width: f64,
    #[arg(long, default_value_t = 9.0)]
    height: f64,
    #[arg(long, default_value_t = 10)]
    plate_count: usize,
    #[arg(long, default_value_t = 10)]
    plate_size: usize,
    #[arg(long, default_value_t = 55)]
    continent_count: usize,
    #[arg(long, default_value_t = 350)]
    continent_size: usize,
    #[arg(long, default_value_t = 66)]
    ocean_count: usize,
    #[arg(long, default_value_t = 250)]
    ocean_size: usize,
    #[arg(long)]
    seed: Option<String>,
}
fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut rng = match args.seed {
        Some(s) => {
            let num = num::BigUint::from_str_radix(&s, 36)?;
            let seed_bytes = num.to_bytes_le();
            let mut seed_arr = [0u8; 32];
            for (i, b) in seed_bytes.iter().enumerate().take(32) {
                seed_arr[i] = *b;
            }
            ChaCha20Rng::from_seed(seed_arr)
        },
        None => ChaCha20Rng::from_os_rng(),
    };
    do_things(
        args.width,
        args.height,
        args.plate_count,
        args.plate_size,
        args.continent_count,
        args.continent_size,
        args.ocean_count,
        args.ocean_size,
        &mut rng,
    )?;
    let seed = rng.get_seed();
    let num = num::BigUint::from_bytes_le(&seed);
    let seed = num.to_str_radix(36);
    println!("Seed: {}", seed);
    

    Ok(())
}
fn do_things<R: Rng + Clone>(
    width: f64,
    height: f64,
    plate_count: usize,
    plate_size: usize,
    continent_count: usize,
    continent_size: usize,
    ocean_count: usize,
    ocean_size: usize,
    mut rng: &mut R,
) -> anyhow::Result<()> {
    let my_voronoi = generate(&mut rng, width, height, plate_count * plate_size)?;

    let mut plates: HashMap<CellId, PlateId> = HashMap::new();

    rng.clone()
        .sample_iter(rand::distr::Uniform::new(0, plate_count * plate_size).unwrap())
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
    let plates_to_cells: HashMap<PlateId, Vec<CellId>> = helpers::invert_borrowed(&plates);
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
    rng.clone()
        .sample_iter(rand::distr::Uniform::new(0, continent_count * continent_size).unwrap())
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
    let last_continent_id = continents
        .values()
        .map(|v| *v)
        .max()
        .unwrap_or(ContinentId(0));
    let mut island_index = 0;
    for cell in continents_voronoi.iter_cells() {
        let cell_id = CellId(cell.site());
        if !continents.contains_key(&cell_id) && !cell.is_on_hull() {
            if rng.random_bool(0.01) {
                let continent_id = ContinentId(last_continent_id.0 + island_index);
                island_index += 1;
                continents.insert(CellId(cell.site()), continent_id);
            }
        } else {
            if cell.is_on_hull() {
                let continent = *continents.get(&cell_id).unwrap_or(&ContinentId(0));
                continents.retain(|_k, v| *v != continent);
            }
        }
    }

    svg::render(
        width,
        height,
        &continents_voronoi,
        &continents,
        "continents_voronoi.svg",
    )?;
    let mut continent_ids = continents
        .values()
        .map(|v| *v)
        .collect::<HashSet<ContinentId>>()
        .iter()
        .map(|v| *v)
        .collect::<Vec<_>>();
    //continent_ids.sort();
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

    svg::render(
        width,
        height,
        &continents_voronoi,
        &continents,
        "continents_voronoi_merged.svg",
    )?;
    Ok(())
}
fn set_neighbour_plate(cell: &VoronoiCell, plates: &mut HashMap<CellId, PlateId>, i: PlateId) {
    for n in cell
        .iter_neighbors()
        .filter(|c| !plates.contains_key(&CellId(*c)))
        .collect::<Vec<_>>()
    {
        let n_id = CellId(n);
        if !plates.contains_key(&n_id) {
            plates.insert(n_id, i);
        }
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
                }) {
                    if plate_id == current_plate_id {
                        continents.insert(n_id, i);
                        continue;
                    }
                }
                if rng.random_bool(0.3) {
                    continents.insert(n_id, i);
                }
            }
        }
    } else {
        return;
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
mod helpers {
    use std::collections::HashMap;
    use std::hash::Hash;

    pub fn invert_borrowed<K, V>(map: &HashMap<K, V>) -> HashMap<V, Vec<K>>
    where
        V: Eq + Hash + Copy,
        K: Eq + Hash + Copy,
    {
        let mut out: HashMap<V, Vec<K>> = HashMap::with_capacity(map.len());
        for (k, v) in map {
            out.entry(*v).or_default().push(*k);
        }
        out
    }
}
mod svg {
    use anyhow::Ok;
    use simple_svg::*;
    use std::{collections::HashMap, env};
    use voronoice::Voronoi;

    use crate::CellId;
    const PALLETTE: [&'static str; 10] = [
        "#FF0000", "#00FF00", "#0000FF", "#FFFF00", "#FF00FF", "#00FFFF", "#FFA500", "#800080",
        "#008000", "#000080",
    ];
    pub fn render_svg<'a, V: Into<usize> + Copy>(
        svg: &mut simple_svg::Svg,
        scale: f64,
        voronoi: &Voronoi,
        plates: &'a HashMap<CellId, V>,
    ) -> anyhow::Result<simple_svg::Group> {
        let mut group = Group::new();
        let mut blue_piece_path_sstyle = Sstyle::new();
        blue_piece_path_sstyle.stroke = Some("#3E5BA9".to_string());
        blue_piece_path_sstyle.stroke_width = Some(0.5);

        for cell in voronoi.iter_cells() {
            if plates.get(&CellId(cell.site())).is_none() {
                continue;
            }
            let circle_id = svg.add_shape(Shape::Circle(Circle::new(0.1)));
            group.place_widget(Widget {
                shape_id: circle_id,
                at: Some((
                    cell.site_position().x * scale,
                    cell.site_position().y * scale,
                )),
                style: Some(blue_piece_path_sstyle.clone()),
                ..Default::default()
            });
            let mut p = simple_svg::Path::new();
            p.moveto_abs((
                cell.site_position().x * scale,
                cell.site_position().y * scale,
            ));
            let mut first = true;
            for vertex in cell.iter_vertices() {
                if first {
                    first = false;
                    p.moveto_abs((vertex.x * scale, vertex.y * scale));
                    continue;
                }
                p.lineto_abs((vertex.x * scale, vertex.y * scale));
            }
            p.close();
            let p_id = svg.add_shape(simple_svg::define::shape::Shape::Path(p));
            let mut style = blue_piece_path_sstyle.clone();
            style.fill_opacity = Some(0.5);
            style.fill = Some(
                PALLETTE
                    [plates.get(&CellId(cell.site())).copied().unwrap().into() % PALLETTE.len()]
                .to_string(),
            );
            group.place_widget(Widget {
                shape_id: p_id,
                style: Some(style),
                ..Default::default()
            });
        }

        return Ok(group);
    }
    pub fn render<'a, V: Into<usize> + Copy>(
        width: f64,
        height: f64,
        voronoi: &Voronoi,
        plates: &'a HashMap<CellId, V>,
        path: &str,
    ) -> anyhow::Result<()> {
        let scale = 50.0;
        let mut svg = simple_svg::Svg::new(width * scale, height * scale);
        let group = render_svg(&mut svg, scale, voronoi, plates)?;
        svg.add_default_group(group);
        let svg = svg_out(svg);

        //let svg = svg::render(&transform, &my_voronoi);
        std::fs::write(env::current_dir()?.join(path), svg)?;
        Ok(())
    }
    // pub fn render_hull_svg(
    //     svg: &mut simple_svg::Svg,
    //     scale: f64,
    //     hulls: &HashMap<PlateId, geo::Polygon>,
    // ) -> anyhow::Result<Group> {
    //     let mut group = Group::new();
    //     for (plate, hull) in hulls.iter() {
    //         let p = simple_svg::Polygon::new(
    //             hull.exterior()
    //                 .coords()
    //                 .map(|c| (c.x * scale, c.y * scale))
    //                 .collect(),
    //         );
    //         let p_id = svg.add_shape(simple_svg::define::shape::Shape::Polygon(p));
    //         let mut style = Sstyle::new();
    //         style.stroke = Some("#000000".to_string());
    //         style.stroke_width = Some(1.0);
    //         style.fill_opacity = Some(1.0);
    //         style.fill = Some(PALLETTE[plate % PALLETTE.len()].to_string());
    //         group.place_widget(Widget {
    //             shape_id: p_id,
    //             style: Some(style),
    //             ..Default::default()
    //         });
    //     }

    //     Ok(group)
    // }
    // pub fn render_hull(
    //     width: f64,
    //     height: f64,
    //     hulls: &HashMap<PlateId, geo::Polygon>,
    //     path: &str,
    // ) -> anyhow::Result<()> {
    //     let scale = 50.0;
    //     let mut svg = simple_svg::Svg::new(width * scale, height * scale);
    //     let group = render_hull_svg(&mut svg, scale, hulls)?;
    //     svg.add_default_group(group);
    //     let svg = svg_out(svg);

    //     //let svg = svg::render(&transform, &my_voronoi);
    //     std::fs::write(env::current_dir()?.join(path), svg)?;
    //     Ok(())
    // }
    // pub fn render_continents_and_plates(
    //     width: f64,
    //     height: f64,
    //     voronoi: &Voronoi,
    //     continents: &HashMap<usize, usize>,
    //     hull_plates: &HashMap<PlateId, geo::Polygon>,
    // ) -> anyhow::Result<()> {
    //     let scale = 50.0;
    //     let mut svg = simple_svg::Svg::new(width * 50.0, height * 50.0);

    //     let plate_group = render_hull_svg(&mut svg, scale, &hull_plates)?;
    //     let continent_group = render_svg(&mut svg, scale, &voronoi, &continents)?;
    //     let mut group = Group::new();
    //     plate_group.widget_list.iter().for_each(|w| {
    //         group.place_widget(w.clone());
    //     });
    //     continent_group.widget_list.iter().for_each(|w| {
    //         group.place_widget(w.clone());
    //     });
    //     svg.add_default_group(group);
    //     let svg = svg_out(svg);
    //     std::fs::write(
    //         std::env::current_dir()?.join("plates_and_continents.svg"),
    //         svg,
    //     )?;
    //     Ok(())
    // }
}

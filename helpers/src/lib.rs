use glam::Vec2;
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
pub fn min_max_componentwise<I>(mut iter: I) -> Option<(Vec2, Vec2)>
where
    I: Iterator<Item = Vec2>,
{
    let first = iter.next()?; // early-return None if empty

    let (min, max) = iter.fold((first, first), |(min, max), v| (min.min(v), max.max(v)));

    Some((min, max))
}

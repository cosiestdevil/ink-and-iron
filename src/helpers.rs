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

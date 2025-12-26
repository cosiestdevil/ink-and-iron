use llm_api::unit_spawn_barks::UnitSpawnBarkCtx;

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    let ctx = UnitSpawnBarkCtx {
        civilisation_name: "Atlantis".to_string(),
        unit_type: "Warrior".to_string(),
        civ_description: "Atlantis Descritpion".to_owned(),
        seed_barks: vec![
            "Hello".to_owned(),
            "Spawned".to_owned(),
            "I AM HERE".to_owned(),
        ],
        description: "Atlantis Warriror".to_owned(),
    };
    let barks = llm_provider::unit_spawn_barks(ctx, 0.5).await?;
    println!("Received barks: {:?}", barks);
    Ok(())
}

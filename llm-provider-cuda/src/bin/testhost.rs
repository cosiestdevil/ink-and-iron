use llm_api::unit_spawn_barks::UnitSpawnBarkCtx;

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    let ctx = UnitSpawnBarkCtx {
        civilisation_name: "Atlantis".to_string(),
        unit_type: "Warrior".to_string(),
        civ_description: "Atlantis Descritpion",
        seed_barks: ["Hello", "Spawned", "I AM HERE"],
        description: "Atlantis Warriror",
    };
    let barks = llm_provider::unit_spawn_barks(ctx, 0.5).await?;
    println!("Received barks: {:?}", barks);
    Ok(())
}

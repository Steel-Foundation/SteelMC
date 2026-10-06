use super::tests::{settings, temp_level_data_dir};
use super::*;
use steel_registry::{init_vanilla_registry, vanilla_dimension_types};

fn generation() -> WorldGenerationSettings {
    settings(
        "minecraft:overworld",
        vanilla_dimension_types::OVERWORLD.height,
    )
}

async fn load(path: &Path, source: GameTimeSource) -> io::Result<LevelDataManager> {
    LevelDataManager::new(Some(path), 7, Difficulty::Normal, generation(), source).await
}

async fn write_time(path: &Path, value: Option<toml::Value>) {
    let mut level_data = LevelData::new_with_seed(7);
    level_data.generation = Some(generation());

    let mut data = toml::Table::try_from(level_data).expect("serialize fixture");
    data.remove("game_time");
    if let Some(value) = value {
        data.insert("game_time".to_owned(), value);
    }

    fs::write(
        path.join("level.toml"),
        toml::to_string(&data).expect("serialize table"),
    )
    .await
    .expect("write fixture");
}

#[tokio::test]
async fn game_time_primary_only_survives_both_shutdown_save_orders() {
    init_vanilla_registry();
    for primary_first in [true, false] {
        let primary_dir = temp_level_data_dir("clock-primary");
        let derived_dir = temp_level_data_dir("clock-derived");
        write_time(&primary_dir, Some(1234.into())).await;
        write_time(&derived_dir, Some(987_654.into())).await;

        let mut primary = load(&primary_dir, GameTimeSource::Primary)
            .await
            .expect("primary");
        let clock = primary.game_time_handle();
        let mut derived = load(&derived_dir, GameTimeSource::Derived(Arc::clone(&clock)))
            .await
            .expect("derived");
        assert!(Arc::ptr_eq(&clock, &derived.game_time_handle()));
        assert_eq!(clock.ticks(), 1234);

        primary.save().await.expect("save initialized primary");
        assert!(!primary.is_dirty());

        primary.advance_game_time();
        if primary_first {
            primary.save().await.expect("save primary");
            derived.save().await.expect("save derived");
        } else {
            derived.save().await.expect("save derived");
            primary.save().await.expect("save primary");
        }

        let saved: toml::Table = toml::from_str(
            &fs::read_to_string(derived_dir.join("level.toml"))
                .await
                .expect("read"),
        )
        .expect("table");
        assert!(!saved.contains_key("game_time"));

        let reloaded = load(&primary_dir, GameTimeSource::Primary)
            .await
            .expect("reload primary");
        assert_eq!(reloaded.game_time_handle().ticks(), 1235);
        load(
            &derived_dir,
            GameTimeSource::Derived(reloaded.game_time_handle()),
        )
        .await
        .expect("reload derived without legacy field");
        assert!(
            load(&derived_dir, GameTimeSource::Primary).await.is_err(),
            "promotion cannot invent authority"
        );

        fs::remove_dir_all(primary_dir).await.expect("cleanup");
        fs::remove_dir_all(derived_dir).await.expect("cleanup");
    }
}

#[tokio::test]
async fn game_time_load_validates_only_the_authoritative_time_and_keeps_other_errors() {
    init_vanilla_registry();
    let dir = temp_level_data_dir("legacy-types");
    let clock = Arc::new(GameTime::new(456));
    for value in [None, Some("obsolete".into())] {
        write_time(&dir, value).await;
        assert!(load(&dir, GameTimeSource::Primary).await.is_err());
        let derived = load(&dir, GameTimeSource::Derived(Arc::clone(&clock)))
            .await
            .expect("legacy value ignored");
        assert_eq!(derived.game_time_handle().ticks(), 456);
    }

    write_time(&dir, Some("obsolete".into())).await;
    let content = fs::read_to_string(dir.join("level.toml"))
        .await
        .expect("read");

    fs::write(
        dir.join("level.toml"),
        content.replace("seed = 7", "seed = false"),
    )
    .await
    .expect("corrupt seed");
    assert!(load(&dir, GameTimeSource::Derived(clock)).await.is_err());

    fs::remove_dir_all(dir).await.expect("cleanup");
}

#[tokio::test]
async fn game_time_wraps_and_persists_the_signed_value() {
    init_vanilla_registry();
    let dir = temp_level_data_dir("wrapping-clock");
    write_time(&dir, Some(i64::MAX.into())).await;
    let mut primary = load(&dir, GameTimeSource::Primary).await.expect("primary");

    primary.advance_game_time();
    primary.save().await.expect("save wrapped value");

    assert_eq!(
        load(&dir, GameTimeSource::Primary)
            .await
            .expect("reload")
            .game_time_handle()
            .ticks(),
        i64::MIN
    );

    fs::remove_dir_all(dir).await.expect("cleanup");
}

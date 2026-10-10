//! Asset commands: import, analysis, proxies, relinking.

use serde_json::{Value, json};
use snapshort_usecases::{AssetCommand, AssetId};

use crate::commands::cmd;
use crate::params::{self, require_uuid, str_p};
use crate::{EngineError, Result, Session};

use super::{has_project, query};

pub fn specs() -> Vec<crate::commands::CommandSpec> {
    vec![
        cmd!(
            "asset.import",
            "Import Media",
            ["File", "Import"],
            Some("Cmd+I"),
            "{\"paths\": [string]}",
            has_project,
            run_import
        ),
        cmd!(
            "asset.analyze",
            "Analyze Media",
            [],
            None::<&'static str>,
            "{\"asset\": id}",
            has_project,
            run_analyze
        ),
        cmd!(
            "asset.generateProxy",
            "Generate Proxy",
            [],
            None::<&'static str>,
            "{\"asset\": id}",
            has_project,
            run_generate_proxy
        ),
        cmd!(
            "asset.delete",
            "Remove Asset",
            [],
            None::<&'static str>,
            "{\"asset\": id}",
            has_project,
            run_delete
        ),
        cmd!(
            "asset.update",
            "Edit Asset Metadata",
            [],
            None::<&'static str>,
            "{\"asset\": id, \"name\"?: string, \"tags\"?: [string], \"rating\"?: 0-5, \"notes\"?: string}",
            has_project,
            run_update
        ),
        cmd!(
            "asset.relink",
            "Relink Media",
            [],
            None::<&'static str>,
            "{\"asset\": id, \"path\": string}",
            has_project,
            run_relink
        ),
        cmd!(
            "asset.relinkFolder",
            "Relink All Missing Media",
            [],
            None::<&'static str>,
            "{\"dir\": string}",
            has_project,
            run_relink_folder
        ),
        cmd!(
            "asset.setProxyPolicy",
            "Proxy Settings",
            [],
            None::<&'static str>,
            "{\"autoGenerate\"?: bool, \"minWidth\"?: int}",
            has_project,
            run_set_policy
        ),
        query!("asset.list", "Assets", "{}", run_list),
    ]
}

fn asset_id(p: &Value, cmd: &str) -> Result<AssetId> {
    require_uuid(p, "asset", cmd).map(AssetId)
}

fn run_import<'a>(s: &'a Session, p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        let paths = params::require_paths(p, "asset.import")?;
        s.dispatch(crate::SessionCommand::Asset(AssetCommand::Import { paths }))
            .await?;
        Ok(crate::inspect::assets_json(s).await)
    })
}

fn run_analyze<'a>(s: &'a Session, p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        let asset_id = asset_id(p, "asset.analyze")?;
        s.dispatch(crate::SessionCommand::Asset(AssetCommand::Analyze {
            asset_id,
        }))
        .await?;
        Ok(crate::inspect::assets_json(s).await)
    })
}

fn run_generate_proxy<'a>(s: &'a Session, p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        let asset_id = asset_id(p, "asset.generateProxy")?;
        s.dispatch(crate::SessionCommand::Asset(AssetCommand::GenerateProxy {
            asset_id,
        }))
        .await?;
        Ok(crate::inspect::assets_json(s).await)
    })
}

fn run_delete<'a>(s: &'a Session, p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        let asset_id = asset_id(p, "asset.delete")?;
        s.dispatch(crate::SessionCommand::Asset(AssetCommand::Delete {
            asset_id,
        }))
        .await?;
        Ok(crate::inspect::assets_json(s).await)
    })
}

fn run_update<'a>(s: &'a Session, p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        let asset_id = asset_id(p, "asset.update")?;
        let name = str_p(p, "name").map(str::to_string);
        let tags = params::optional_object::<Vec<String>>(p, "tags", "asset.update")?;
        let rating = params::u32_p(p, "rating")
            .map(|r| r.min(5) as u8)
            .or_else(|| params::f64_p(p, "rating").map(|r| r.round().clamp(0.0, 5.0) as u8));
        s.dispatch(crate::SessionCommand::Asset(AssetCommand::UpdateMetadata {
            asset_id,
            name,
            tags,
            rating,
        }))
        .await?;
        Ok(crate::inspect::assets_json(s).await)
    })
}

fn run_relink<'a>(s: &'a Session, p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        let asset_id = asset_id(p, "asset.relink")?;
        let new_path = params::require_paths(p, "asset.relink")?
            .pop()
            .ok_or_else(|| EngineError::bad("asset.relink", "`path` is required"))?;
        s.dispatch(crate::SessionCommand::Asset(AssetCommand::Relink {
            asset_id,
            new_path,
        }))
        .await?;
        Ok(crate::inspect::assets_json(s).await)
    })
}

fn run_relink_folder<'a>(s: &'a Session, p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        let dir = params::require_paths(p, "asset.relinkFolder")?
            .pop()
            .ok_or_else(|| EngineError::bad("asset.relinkFolder", "`dir` is required"))?;
        s.dispatch(crate::SessionCommand::Asset(AssetCommand::RelinkInFolder {
            dir,
        }))
        .await?;
        Ok(crate::inspect::assets_json(s).await)
    })
}

fn run_set_policy<'a>(s: &'a Session, p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        let current = s.services().assets.proxy_policy().await;
        let auto_generate = params::bool_p(p, "autoGenerate").unwrap_or(current.auto_generate);
        let min_width = params::u32_p(p, "minWidth")
            .unwrap_or(current.min_width)
            .max(1);
        s.dispatch(crate::SessionCommand::Asset(AssetCommand::SetProxyPolicy {
            auto_generate,
            min_width,
        }))
        .await?;
        Ok(json!({
            "ok": true,
            "autoGenerate": auto_generate,
            "minWidth": min_width,
        }))
    })
}

fn run_list<'a>(s: &'a Session, _p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move { Ok(crate::inspect::assets_json(s).await) })
}

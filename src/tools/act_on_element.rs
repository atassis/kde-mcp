use crate::atspi::ref_encoding::decode_element_ref;
use crate::error::ToolError;
use crate::refs::{RefError, RefKind};
use crate::server::KdeMcp;
use atspi::proxy::accessible::AccessibleProxy;
use atspi::proxy::proxy_ext::ProxyExt;
use atspi::zbus::names::BusName;
use atspi::zbus::zvariant::ObjectPath;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ActOnElementArgs {
    pub element_ref: String,
    pub action: String,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ActOnElementResponse {
    pub ok: bool,
    pub action_invoked: String,
}

pub async fn act_on_element(
    state: &KdeMcp,
    args: ActOnElementArgs,
) -> Result<ActOnElementResponse, ToolError> {
    let meta = state
        .refs
        .resolve(&args.element_ref)
        .await
        .map_err(|e| match e {
            RefError::Stale | RefError::Unknown => ToolError::StaleHandle {
                ref_id: args.element_ref.clone(),
            },
        })?;
    if meta.kind != RefKind::Element {
        return Err(ToolError::UnsupportedRegion {
            reason: format!("ref {} is not an element", args.element_ref),
        });
    }
    let (bus, path) = decode_element_ref(&meta.payload).ok_or_else(|| ToolError::StaleHandle {
        ref_id: args.element_ref.clone(),
    })?;

    let conn = state.atspi.get().await?;
    let bus_name = BusName::try_from(bus.to_string()).map_err(|e| ToolError::StaleHandle {
        ref_id: format!("bad bus name {bus}: {e}"),
    })?;
    let obj_path = ObjectPath::try_from(path.to_string()).map_err(|e| ToolError::StaleHandle {
        ref_id: format!("bad path {path}: {e}"),
    })?;

    let acc = AccessibleProxy::builder(conn.connection())
        .destination(bus_name)
        .map_err(|e| ToolError::BusDisconnected {
            detail: e.to_string(),
        })?
        .path(obj_path)
        .map_err(|e| ToolError::BusDisconnected {
            detail: e.to_string(),
        })?
        .cache_properties(atspi::zbus::proxy::CacheProperties::No)
        .build()
        .await
        .map_err(|e| ToolError::BusDisconnected {
            detail: e.to_string(),
        })?;

    let proxies = acc
        .proxies()
        .await
        .map_err(|e| ToolError::BusDisconnected {
            detail: e.to_string(),
        })?;
    let action_proxy = proxies
        .action()
        .await
        .map_err(|_| ToolError::NotActionable {
            available_actions: vec![],
            fallback_suggestion: Some(
                "element has no Action interface; try mouse.click on bbox".into(),
            ),
        })?;
    let actions = action_proxy
        .get_actions()
        .await
        .map_err(|e| ToolError::BusDisconnected {
            detail: e.to_string(),
        })?;

    let requested = args.action.to_ascii_lowercase();
    let idx = actions
        .iter()
        .position(|a| a.name.eq_ignore_ascii_case(&requested))
        .ok_or_else(|| ToolError::NotActionable {
            available_actions: actions.iter().map(|a| a.name.clone()).collect(),
            fallback_suggestion: Some(format!(
                "no action named {requested:?}; try one of available_actions"
            )),
        })? as i32;

    let invoked = action_proxy
        .do_action(idx)
        .await
        .map_err(|e| ToolError::BusDisconnected {
            detail: e.to_string(),
        })?;
    if !invoked {
        return Err(ToolError::NotActionable {
            available_actions: actions.iter().map(|a| a.name.clone()).collect(),
            fallback_suggestion: Some("AT-SPI do_action returned false".into()),
        });
    }
    Ok(ActOnElementResponse {
        ok: true,
        action_invoked: actions[idx as usize].name.clone(),
    })
}

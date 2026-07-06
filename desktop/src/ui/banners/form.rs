
use slint::{Image, Model, ModelRc, SharedString};
use uuid::Uuid;

use letaf_core::banner::model::Banner;

use crate::{BannerData, MainWindow};

use super::super::image::decode_pixel_buffer;
use super::crud::validate_url;

// ── Helpers ──────────────────────────────────────────────

pub(crate) struct BannerForm {
    pub(crate) title: String,
    pub(crate) image_data: String,
    pub(crate) item_type: String,
    pub(crate) item_id: Option<Uuid>,
    pub(crate) item_url: Option<String>,
}

/// Lê o form da UI e valida. Atualiza erros inline e devolve `None`
/// quando algum campo estiver inválido — assim o caller sai cedo sem
/// chamar o service.
pub(crate) fn read_and_validate(ui: &MainWindow) -> Option<BannerForm> {
    // Limpa erros antes de revalidar.
    ui.set_banner_error_title(SharedString::default());
    ui.set_banner_error_image(SharedString::default());
    ui.set_banner_error_target(SharedString::default());

    let title = ui.get_banner_title().to_string();
    let image_data = ui.get_banner_image_data().to_string();
    let item_type = ui.get_banner_item_type().to_string();

    let mut ok = true;
    if title.trim().is_empty() {
        ui.set_banner_error_title(SharedString::from("Informe o título"));
        ok = false;
    }
    if image_data.trim().is_empty() {
        ui.set_banner_error_image(SharedString::from("Envie uma imagem (3:1)"));
        ok = false;
    }

    let (item_id, item_url) = match item_type.as_str() {
        "product" => {
            let pid = ui.get_banner_product_id().to_string();
            match Uuid::parse_str(&pid) {
                Ok(uuid) => (Some(uuid), None),
                Err(_) => {
                    ui.set_banner_error_target(SharedString::from("Selecione um produto"));
                    ok = false;
                    (None, None)
                }
            }
        }
        "url" => {
            let url = ui.get_banner_url().to_string();
            match validate_url(&url) {
                Some(msg) => {
                    ui.set_banner_error_target(SharedString::from(msg));
                    ok = false;
                    (None, None)
                }
                None => (None, Some(url.trim().to_string())),
            }
        }
        _ => (None, None),
    };

    if !ok { return None; }

    Some(BannerForm { title, image_data, item_type, item_id, item_url })
}

pub(crate) fn clear_form(ui: &MainWindow) {
    ui.set_editing_id(SharedString::default());
    ui.set_banner_title(SharedString::default());
    ui.set_banner_item_type(SharedString::from("product"));
    ui.set_banner_product_id(SharedString::default());
    ui.set_banner_product_name(SharedString::default());
    ui.set_banner_url(SharedString::default());
    ui.set_banner_image_data(SharedString::default());
    ui.set_banner_error_title(SharedString::default());
    ui.set_banner_error_image(SharedString::default());
    ui.set_banner_error_target(SharedString::default());
}

/// Converte `Banner` (domínio) → `BannerData` (Slint), resolvendo o
/// nome do produto vinculado a partir da lista atual de produtos e
/// já decodificando o base64 para `slint::Image` (miniatura na lista).
/// Produto desconhecido (id que não está mais na lista) cai em "—".
pub(crate) fn to_banner_data(b: &Banner, products: &ModelRc<crate::ProductData>) -> BannerData {
    let item_id_str = b.item_id.map(|u| u.to_string()).unwrap_or_default();
    let item_name = if !item_id_str.is_empty() {
        products.iter()
            .find(|p| p.id == item_id_str.as_str())
            .map(|p| p.name.to_string())
            .unwrap_or_else(|| "".to_string())
    } else {
        String::new()
    };
    let image = decode_pixel_buffer(&b.image_data)
        .map(Image::from_rgba8)
        .unwrap_or_default();
    BannerData {
        id: SharedString::from(b.base.id.to_string()),
        title: SharedString::from(b.title.as_str()),
        image_data: SharedString::from(b.image_data.as_str()),
        image,
        item_type: SharedString::from(b.item_type.as_str()),
        item_id: SharedString::from(item_id_str),
        item_name: SharedString::from(item_name),
        item_url: SharedString::from(b.item_url.as_deref().unwrap_or("")),
        active: b.active,
        sort_order: b.sort_order,
    }
}

//! Preenchimento das miniaturas de produto em segundo plano.
//!
//! `thumb_data` é DERIVADA de `image_data` e nasce vazia nos produtos
//! cadastrados antes da coluna existir — e também nos que chegam pelo sync de
//! um terminal que ainda não tinha o campo. Sem miniatura, a lista cai no
//! fallback e volta a carregar a imagem inteira, que é justamente o custo que
//! se quis eliminar.
//!
//! Roda uma vez por boot, fora do caminho da UI (§7.4: nada bloqueia a
//! interface). Decodificar e reescalar imagem é CPU-bound, então vai em
//! `spawn_blocking` — segurar o executor do Tokio travaria o sync junto.

use std::time::Duration;

use crate::context::DesktopState;
use crate::ui::image::make_thumbnail;

/// Espera antes de começar: deixa o boot, o primeiro render e o primeiro
/// ciclo de sync passarem na frente.
const ATRASO_INICIAL_SECS: u64 = 20;

/// Gera as miniaturas que faltam. Falha em um produto não interrompe os
/// demais — miniatura ausente degrada o desempenho, não a correção.
pub fn spawn_backfill(state: DesktopState) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(ATRASO_INICIAL_SECS)).await;
        let cid = state.company_id();
        let pendentes = match state.product_service.find_sem_miniatura(cid).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("miniaturas: não listou os pendentes: {e}");
                return;
            }
        };
        if pendentes.is_empty() {
            return;
        }
        tracing::info!("miniaturas: gerando para {} produto(s)", pendentes.len());
        let mut feitas = 0u32;
        for p in pendentes {
            let Some(img) = p.image_data.clone() else { continue };
            // CPU-bound fora do executor.
            let thumb = match tokio::task::spawn_blocking(move || make_thumbnail(&img)).await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("miniaturas: tarefa falhou: {e}");
                    continue;
                }
            };
            let Some(thumb) = thumb else { continue };
            match state
                .product_service
                .set_thumbnail(cid, p.base.id, Some(&thumb))
                .await
            {
                Ok(()) => feitas += 1,
                Err(e) => tracing::warn!("miniaturas: não gravou {}: {e}", p.base.id),
            }
        }
        tracing::info!("miniaturas: {feitas} gerada(s)");
    });
}

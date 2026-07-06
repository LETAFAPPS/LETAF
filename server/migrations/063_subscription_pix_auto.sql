-- Pix Automático na assinatura (mandato de débito recorrente do BACEN)
-- — espelha desktop/063.
--
-- Regras aplicadas (AI_RULES.md §6, §11):
-- - `pix_auto_rec_id` = idRec da recorrência no gateway (referência
--   opaca). `pix_auto_status` reflete o estado do mandato.
-- - Reutiliza `gateway` (já criada na 062).
ALTER TABLE subscriptions ADD COLUMN pix_auto_rec_id TEXT;
ALTER TABLE subscriptions ADD COLUMN pix_auto_status TEXT;

-- Lookup por idRec ao processar o webhook do Pix Automático.
CREATE INDEX idx_subscriptions_pix_auto_rec
    ON subscriptions(pix_auto_rec_id)
    WHERE pix_auto_rec_id IS NOT NULL;

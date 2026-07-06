-- Pix Automático na assinatura — espelha server/063.
--
-- Regras aplicadas (AI_RULES.md §6, §11):
-- - `pix_auto_rec_id` (idRec) + `pix_auto_status`, sincronizados a
--   partir do servidor (pull). O mandato é criado online.
ALTER TABLE subscriptions ADD COLUMN pix_auto_rec_id TEXT;
ALTER TABLE subscriptions ADD COLUMN pix_auto_status TEXT;

CREATE INDEX idx_subscriptions_pix_auto_rec
    ON subscriptions(pix_auto_rec_id)
    WHERE pix_auto_rec_id IS NOT NULL;

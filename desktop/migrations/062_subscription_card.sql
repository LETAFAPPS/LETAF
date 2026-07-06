-- Cartão recorrente na assinatura — espelha server/062.
--
-- Regras aplicadas (AI_RULES.md §6, §11):
-- - Sem dados sensíveis (PAN/CVV). Só o `gateway_subscription_id` e o
--   status do gateway, sincronizados a partir do servidor (pull).
-- - O cartão é cadastrado online (server tokeniza na Efi); estas
--   colunas só refletem o resultado localmente.
ALTER TABLE subscriptions ADD COLUMN gateway TEXT;
ALTER TABLE subscriptions ADD COLUMN gateway_subscription_id TEXT;
ALTER TABLE subscriptions ADD COLUMN card_status TEXT;

CREATE INDEX idx_subscriptions_gateway_sub
    ON subscriptions(gateway_subscription_id)
    WHERE gateway_subscription_id IS NOT NULL;

-- Cartão recorrente na assinatura (motor de assinaturas do gateway)
-- — espelha desktop/062.
--
-- Regras aplicadas (AI_RULES.md §6, §11):
-- - Sem dados sensíveis: o PAN/CVV nunca chegam ao banco. Guardamos só
--   o `gateway_subscription_id` (referência opaca) + status do gateway.
--   O rótulo do cartão ("VISA •••• 4242") vive em payment_method_label.
-- - `gateway` permite trocar/adicionar PSP sem migração nova.
ALTER TABLE subscriptions ADD COLUMN gateway TEXT;
ALTER TABLE subscriptions ADD COLUMN gateway_subscription_id TEXT;
ALTER TABLE subscriptions ADD COLUMN card_status TEXT;

-- Lookup por ID do gateway ao processar notificações (webhook).
CREATE INDEX idx_subscriptions_gateway_sub
    ON subscriptions(gateway_subscription_id)
    WHERE gateway_subscription_id IS NOT NULL;

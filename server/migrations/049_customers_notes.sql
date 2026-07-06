-- Observação interna do estabelecimento sobre o cliente (Fase 9).
-- Texto livre, nunca exibido ao cliente final. Sincroniza normalmente
-- (last-write-wins por updated_at).
ALTER TABLE customers ADD COLUMN notes TEXT NULL;

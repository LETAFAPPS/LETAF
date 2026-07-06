-- Janela de disponibilidade do produto no cardápio web, por dia da semana.
-- Armazenada como JSON em TEXT: `[{day, open, close, active}, ...]`.
-- `NULL` = sempre disponível (compatível com produtos legados).
ALTER TABLE products
    ADD COLUMN IF NOT EXISTS availability_schedule TEXT;

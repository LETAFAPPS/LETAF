-- Disponibilidade do produto por dia da semana (JSON em TEXT).
-- NULL = sempre disponível.
ALTER TABLE products
    ADD COLUMN availability_schedule TEXT;

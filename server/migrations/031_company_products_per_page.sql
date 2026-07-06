-- Adiciona quantidade de produtos por página exibida na grade.
-- Default 20. Permite ao admin configurar via tela de Configurações.
ALTER TABLE companies
    ADD COLUMN IF NOT EXISTS products_per_page INTEGER NOT NULL DEFAULT 20;

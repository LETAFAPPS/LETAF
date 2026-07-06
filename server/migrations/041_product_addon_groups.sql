-- Tabela de junção N:M entre produtos e grupos de adicionais (Fase 4).
--
-- O produto pode usar vários grupos ("Borda", "Tamanho"); um grupo
-- pode ser reusado em vários produtos (permite editar uma vez e
-- propagar a todos os usos).
--
-- Sem `synced`/`deleted_at`: a associação é totalmente regravada pelo
-- `Product.update` (reescreve a lista atual) — last-write-wins do
-- produto já governa o estado das associações.
CREATE TABLE IF NOT EXISTS product_addon_groups (
    company_id  UUID NOT NULL REFERENCES companies(id),
    product_id  UUID NOT NULL REFERENCES products(id),
    group_id    UUID NOT NULL REFERENCES addon_groups(id),
    sort_order  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (company_id, product_id, group_id)
);

CREATE INDEX IF NOT EXISTS idx_product_addon_groups_product
    ON product_addon_groups(company_id, product_id);
CREATE INDEX IF NOT EXISTS idx_product_addon_groups_group
    ON product_addon_groups(company_id, group_id);

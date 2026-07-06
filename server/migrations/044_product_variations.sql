-- Variações do produto (Fase 5) — per-produto, em JSON.
--
-- Por que JSON em vez de tabela: variações são pequenas (1-3 por
-- produto, 3-5 opções cada) e NUNCA reusadas entre produtos —
-- "Tamanho" do Hambúrguer ≠ "Tamanho" da Pizza. Tabela separada
-- forçaria sync N:M e join sem ganho. Mesmo padrão de
-- `discount_tiers` e `availability_schedule`.
--
-- Estrutura: array de `{title, selection, required, options:
-- [{name, price}]}` onde `selection` ∈ {single, multi, max_value}.
ALTER TABLE products
    ADD COLUMN IF NOT EXISTS variations TEXT;

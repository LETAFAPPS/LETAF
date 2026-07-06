# Débito técnico — dinheiro em `f64` → `Decimal`

**Status:** pendente (projeto dedicado). Registrado pela auditoria (item #4).
**Data:** 2026-07-06.

## Problema

Valores monetários no domínio usam `f64` (ponto flutuante binário), que não
representa exatamente frações decimais (ex.: `0.1 + 0.2 != 0.3`). Em somatórios
de itens, descontos percentuais, juros e parcelas, isso pode acumular erro de
centavos.

**Mitigação atual:** o helper `round_2`/arredondamento nos limites de cálculo
reduz o erro prático; não há, hoje, imprecisão observada em produção. Por isso
este item é **melhoria de robustez**, não bug ativo.

## Por que NÃO foi feito numa varredura

A conversão provou ser um refactor **all-or-nothing** de ~300 edições em código
financeiro, atravessando os 4 crates:

- **core:** ~48 campos de model + ~45 parâmetros/retornos de função; reescrita
  de `discount.rs`, cálculos de `order`, `cash`, `wallet`, `coupon`,
  `subscription`, `finance`, addons. (~80 erros de compilação só no core após a
  troca de tipos, em cascata: comparações `< 0.0`, literais `0.0`,
  `Decimal * f64` em juros/parcela, `is_finite` inexistente em `Decimal`.)
- **server:** colunas `NUMERIC` (sqlx suporta `Decimal` nativo), Row structs,
  binds, e DTOs do catálogo convertendo `Decimal → f64` na fronteira pública.
- **desktop:** SQLite **não tem tipo decimal** → colunas de dinheiro viram
  `TEXT`, com Row `String → Decimal::from_str` e bind `.to_string()` em todos os
  repositórios; UI Slint (formatação e propriedades numéricas).

Sem **testes** cobrindo a matemática de dinheiro, concluir isso às cegas é mais
arriscado do que o `f64` atual: um erro sutil em desconto/juros/parcela é pior
que a imprecisão que se quer corrigir.

## Plano recomendado (quando for feito)

1. **Testes primeiro:** unidade para `discount::effective_unit_price`, total do
   pedido, cupom (`fixed`/`percent`/limites), juros/parcela da assinatura,
   saldo da carteira e resumo de caixa — travando o comportamento atual.
2. **Tipo:** `rust_decimal::Decimal` nos campos de dinheiro; **quantidades
   seguem `f64`** (peso fracionário não é dinheiro). Produto preço×quantidade
   converte a quantidade com um helper `money::qty`.
3. **Persistência:** Postgres `NUMERIC`; SQLite `TEXT` (string decimal exata —
   `REAL` truncaria para float). Row structs convertem na fronteira.
4. **Serde:** `Decimal` com a mesma configuração em todos os crates → sync
   consistente entre desktop e servidor.
5. **Fronteira web:** o endpoint `/catalog` converte `Decimal → f64` no DTO
   público (o web é cliente "burro", §11 recalcula no checkout) — o crate `web`
   fica intocado.
6. **Compatibilidade:** muda o formato do payload de sync. Requer **limpar os
   bancos** (fresh start) OU uma migração de sync versionada, para não quebrar
   clientes desktop já publicados (`release.yml`).

## Escopo NÃO incluído

Quantidades (`quantity`, `stock_quantity`, `min_stock`, `discount_min_qty`) e
percentuais de uso interno seguem `f64` — não são dinheiro.

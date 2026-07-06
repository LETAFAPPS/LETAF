# Dinheiro em `Decimal` — IMPLEMENTADO

**Status:** ✅ concluído (auditoria #4). Substitui o antigo débito técnico.

Todo valor monetário do domínio agora é `rust_decimal::Decimal` (exato), em vez
de `f64`. Quantidades (peso/unidades) seguem `f64` — não são dinheiro.

## Como ficou

- **core:** campos e cálculos de dinheiro em `Decimal`; helpers em
  `core::money` (`qty`, `round2`, `to_cents`, `from_db_f64`). Descontos, totais,
  cupom, caixa, carteira, assinaturas e juros/parcelas recomputados em Decimal.
  Cobertura de testes (desconto, cupom, resumo de caixa) travando o comportamento.
- **server (Postgres):** colunas de dinheiro `NUMERIC(14,2)`; sqlx decodifica
  `Decimal` nativamente. DTOs públicos do catálogo convertem `Decimal → f64` na
  fronteira (o `web` é cliente "burro" e permanece intocado; §11).
- **desktop (SQLite):** SQLite não tem tipo decimal. O cache local guarda
  dinheiro como `REAL` e converte na fronteira do repositório
  (`money::from_db_f64` na leitura, com `round2`; `.to_f64()` no bind). Os
  CÁLCULOS são exatos (Decimal); só o armazenamento local passa por `f64`, e o
  `round2` elimina o ruído para os 2 decimais. O servidor (NUMERIC) é a fonte
  exata; o desktop sincroniza com ele.
- **serde:** `Decimal` com a mesma configuração em core/server/desktop → sync
  consistente. Bancos foram limpos (fresh start), sem clientes antigos a quebrar.

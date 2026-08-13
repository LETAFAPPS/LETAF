# Redesign do Web — tema Loja (e-commerce)

Proposta visual (planejamento + mockups) para repaginar o catálogo web (crate `web`,
Leptos SSR) no tema **loja**, inspirada em e-commerces modernos, **mantendo a identidade
azul do LETAF** (`--brand: #2563eb`) e o modelo real: uma loja por subdomínio, frontend
burro e SEO no servidor (AI_RULES §3/§11). É o **mesmo web** do cardápio, só re-tematizado
via `data-theme="loja"`.

> Status: **somente planejamento**. Nada foi aplicado ao código. Aguardando aprovação.

## Arquivos

- [`plano.html`](plano.html) — documento visual completo e **self-contained** (abre no
  navegador; imagens embutidas). Design system, mockups, plano por componente, mapeamento
  Hexacom→LETAF, escopo e roadmap. Também publicado como Artifact.
- [`mockups/`](mockups/) — as 6 telas em alta resolução (geradas por IA, ilustrativas):
  - `loja_home_light.png` — loja, tema claro
  - `loja_home_dark.png` — loja, tema escuro
  - `loja_product_modal.png` — modal de produto (galeria + preço)
  - `loja_cart.png` — carrinho (drawer) com entrega/retirada e totais
  - `loja_mobile.png` — loja no celular
  - `loja_login.png` — modal de entrar/cadastrar

## Escopo (resumo)

- **Adotar**: cards com foto + selo de desconto + preço riscado/atual + favoritar/carrinho,
  categorias em círculos, banners de oferta, lista de desejos (`favorites.rs` já existe),
  carrinho com Entrega/Retirada + cupom + totais.
- **Adaptar**: login ao que o backend suporta; manter modal de produto (não página);
  linhas por categoria; "similares" só se houver dado.
- **Fora de escopo**: estrelas/avaliações (backend novo), multi-loja, i18n e apps mobile.

## Roadmap (após aprovação)

1. Fundação — tokens & tipografia (SCSS do tema loja + modo escuro)
2. Header + categorias (círculos) + card de produto
3. Modal do produto + carrinho (entrega/retirada, cupom, totais)
4. Favoritos + Entrar/Conta + estados
5. Acabamento — microinterações, acessibilidade, QA responsivo, SEO/SSR

## Observação sobre os mockups

Imagens geradas por IA para dar a sensação do visual — layout, paleta e componentes são a
proposta concreta, mas textos/detalhes podem ter pequenas imperfeições. Na implementação
real (Leptos + SCSS) tudo vem de tokens e fica preciso.

Ver também o tema irmão: [`../redesign-cardapio-web/`](../redesign-cardapio-web/) (restaurante).

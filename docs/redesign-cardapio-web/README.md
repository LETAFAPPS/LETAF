# Redesign do Cardápio Web — tema Restaurante

Proposta visual (planejamento + mockups) para repaginar o cardápio web (crate `web`,
Leptos SSR) inspirada em apps modernos de delivery, **mantendo a identidade laranja do
LETAF** e o modelo real do sistema: um restaurante por subdomínio, frontend burro e SEO
no servidor (AI_RULES §3/§11).

> Status: **somente planejamento**. Nada foi aplicado ao código. Aguardando aprovação
> para iniciar pela Fase 1 (tokens).

## Arquivos

- [`plano.html`](plano.html) — documento visual completo e **self-contained** (abre no
  navegador; imagens já embutidas). Contém: design system, mockups, plano por componente,
  mapeamento eFood→LETAF, escopo e roadmap. Também publicado como Artifact.
- [`mockups/`](mockups/) — as 6 telas em alta resolução (geradas por IA, ilustrativas):
  - `mock_home_light.png` — cardápio, tema claro
  - `mock_home_dark.png` — cardápio, tema escuro
  - `mock_product_modal.png` — modal de produto (configurador)
  - `mock_cart.png` — carrinho (drawer) com totais/cupom
  - `mock_mobile.png` — cardápio no celular
  - `mock_login.png` — modal de entrar/cadastrar

## Escopo (resumo)

- **Adotar** a linguagem visual: cards com foto + botão "Adicionar" flutuante + selo de
  desconto, categorias em tiles, banner hero, modal de produto, carrinho com cupom/totais.
- **Adaptar** login e impostos ao que o backend suporta.
- **Fora de escopo** (marketplace, não single-tenant): descoberta de restaurantes,
  "Nearest Branch"/mapa, landing multi-restaurante, selo Veg/estrelas de avaliação.

## Roadmap (após aprovação)

1. Fundação — tokens & tipografia (SCSS do tema restaurante + modo escuro)
2. Header + categorias (tiles) + card de produto
3. Modal do produto + carrinho (drawer)
4. Entrar/Conta + estados (skeleton, vazio, loja fechada)
5. Acabamento — microinterações, acessibilidade, QA responsivo, SEO/SSR

## Observação sobre os mockups

As imagens foram geradas por IA para dar a sensação do visual — layout, paleta e
componentes são a proposta concreta, mas textos/detalhes podem ter pequenas imperfeições.
Na implementação real (Leptos + SCSS) tudo vem de tokens e fica preciso.

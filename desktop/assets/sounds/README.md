# Sons do desktop

Coloque aqui o arquivo `alarm.mp3` que será **embutido no executável**
(`include_bytes!`) e tocado pelo `AlarmPlayer` quando um pedido novo
chegar.

Requisitos:
- Formato: MP3
- Nome exato: `alarm.mp3`
- Caminho: `desktop/assets/sounds/alarm.mp3`

O `AlarmPlayer` (`desktop/src/alarm/player.rs`) carrega o conteúdo via
`include_bytes!("../../assets/sounds/alarm.mp3")` e toca em loop até
o operador clicar em "Ver Pedidos" — ou seja, o áudio inteiro repete
sem pausa. Se o som já tem cadência própria (silêncio entre repetições
incorporado), deixe-o curto; rodio faz o looping perfeitamente.

Para trocar o som, basta substituir o arquivo e recompilar o desktop.
Nenhuma outra mudança no código é necessária.

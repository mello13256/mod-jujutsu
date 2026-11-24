# Novidades do Jujutsu Horizon

## 0.31.0 — 2026-11-24

- **Formato novo do Horizon.** O mod agora usa a API de mods do Horizon (o `horizon-mod.toml` na raiz) e compila com o launcher do Horizon 0.2.108 ou mais novo, contra o SDK pronto do Nightly de 22/11: ninguém precisa mais do launcher 0.2.69 nem de compilar o código do Horizon. Instale por **Mods > Adicionar do GitHub** no launcher do Horizon (veja "Como instalar" no README). O Jujutsu Launcher 2.1 não lê esse formato: a versão nova dele vem em seguida.
- Como o mod usa só a API, ele continua compilando nos Nightlies mais novos enquanto o que ele usa não mudar.
- **Desenho refeito com a API:** os traços, os cortes, a flecha de fogo, as esferas do Gojo, as ondas, o Black Flash, a aura e as explosões são desenhados pelos shaders do próprio mod. Os efeitos que entortavam a imagem do mundo (a lente do Azul, o vidro do Mugen), o modelo do santuário e a Nuke completa vão para um pacote visual separado, que vem depois.
- **Menu (N):** os painéis continuam no mundo e a mira escolhe o botão; o desenho é feito no HUD, com a fonte do jogo.
- **Teclas** agora ficam em **Opções > Controles** (grupo Jujutsu Horizon), e as opções (destruição de blocos, efeitos da explosão) em **Opções > Mods > Jujutsu Horizon > Config**, além do painel CONFIG do menu.
- **Ainda não voltou:** o corpo posado pelas técnicas e os braços em primeira pessoa (o jogo desenha o jogador normal), as feridas desenhadas nos mobs e a inclinação da câmera no pouso da teia. A jogabilidade é a mesma.

## 0.30.3 — 2026-11-23

- **Compila em quem instalou um Horizon mais novo.** Os Nightlies mais novos (o de 22/11, por exemplo) mudaram as pastas do código do Horizon, e o launcher 0.2.69 não consegue montar o código deles ("failed to read ... crates\horizon-mods\Cargo.toml"). O mod agora fixa o Nightly em que foi feito (13/11, 9f42064), e o launcher compila nele seja qual for o Nightly instalado.

## 0.30.2 — 2026-11-20

- **Correção grave: o progresso não era gravado.** No Horizon, a pasta do mundo só existe do lado do servidor do mundo, e o mod a pedia do lado da janela, onde ela não existe; o progresso ficava só na memória e sumia ao fechar o jogo. Agora o servidor conta a pasta e o progresso é gravado em `sukuna/progresso.txt` dentro do mundo, como devia.
- O progresso também é gravado na hora ao pausar (Esc) e ao sair do mundo, não só a cada cinco segundos, e o arquivo é trocado de uma vez (um jogo fechado no meio não deixa um arquivo pela metade).

## 0.30.1 — 2026-11-19

- **Mugen refeito**: o tempo de tudo o que se aproxima passa mais devagar na proporção do que falta até encostar. Os mobs andam, viram e atacam devagar (os ticks deles são pulados) e, com você parado, nunca chegam: param antes de o golpe deles te alcançar. Flechas e outros projéteis perdem a velocidade e ficam no ar. Se você anda para cima de algo, ele é posto de volta.
- **Aura nova**: a fumaça saiu. No lugar dela, o ar em volta do corpo tremula em ondinhas pretas e da cor da energia (azul com o Gojo, vermelha com o Sukuna, verde com a técnica reversa), o efeito que aparecia em volta do corpo com o Mugen.

## 0.30.0 — 2026-11-19

- **Novo nome: Jujutsu Horizon.** O comando agora é `/jujutsu` (o `/sukuna` continua valendo). Mundos e saves de antes continuam funcionando.
- **Black Flash**: logo depois de um soco acertar, aperte M1 e M2 juntos na janela curtinha do impacto. Dano extra, empurrão e atordoamento. No ar, sai em raios negros mais espalhados que dão um dano mínimo nos mobs que tocam.
- **Vazio Ilimitado**: a barreira por fora é preta, os blocos de fora não aparecem mais lá dentro e o Gojo não consegue sair da barreira. Quando o domínio acaba, os mobs ficam mais 2 segundos paralisados.
- **Roxo instável** (Vermelho V1 contra o Azul V1 parado): tremor leve quando vira roxo, cresce até 50% maior em 4 segundos, solta raios roxos até explodir, e a explosão destrói blocos.
- **Roxo**: basta apertar R+V uma vez (não precisa mais do toque duplo), e a esfera que você mira é a mesma que sai.
- **Vermelho clássico**: a explosão acontece ao redor do jogador e destrói blocos.
- **Azul V1**: além de puxar os blocos, destrói os blocos que já absorveu.
- **Pactos novos**: "Sem Vazio Roxo" (Gojo), "Sem Infinito" (Gojo), "Sem Cleave" (Sukuna) e "Corpo como preço" (metade da vida por 5 minutos, +35% de dano). Os pactos de um personagem não aparecem mais para o outro.
- **Animações do Gojo**: mão na frente do rosto com a palma para cima e o poder flutuando acima dela (Azul e Vermelho); o lançamento vai até a pose do R; pose própria para o Azul V1 e para o Roxo.
- **Menu**: a opção virou "Destruição de blocos" e vale para o Sukuna e para o Gojo (só o Azul sem variante não destrói nada).
- **HUD**: os tempos de espera aparecem embaixo da barra azul também para o Gojo.
- **Jujutsu Launcher 2.0**: interface nova, baixa o que falta (o launcher do Horizon 0.2.69, Java, Rust) e mostra estas novidades.

## 0.29.0

- Árvore de habilidades do Gojo: o Vermelho pede a Técnica Reversa; o Roxo pede o Azul e o Vermelho.
- Repositório no GitHub com atualização automática pelo launcher.

## 0.28.0

- Expansão de domínio do Gojo: Vazio Ilimitado.

# Jujutsu Horizon

Feitiçaria de Jujutsu Kaisen no Horizon (Minecraft: Java Edition 26.3 em Rust): o **Sukuna**
(Desmantelar, Cleave, Flecha de fogo, Santuário Malevolente) e o **Gojo** (Azul, Vermelho, Roxo,
Mugen, Vazio Ilimitado), com sistema de nível, Portões, pactos, combate de mão vazia e Black
Flash. Mod nativo, só singleplayer por enquanto. O que mudou em cada versão está no
`CHANGELOG.md`.

Até a 0.29 o mod se chamava "Sukuna". O id continua `sukuna` (a pasta `sukuna/` dentro do mundo
também), para os mundos e saves antigos continuarem valendo. O comando agora é **/jujutsu**; o
**/sukuna** continua funcionando.

## Escolha do personagem (primeira vez que aperta N num mundo)

Num mundo novo, a primeira vez que você aperta **N** o menu mostra só uma janela: **Sukuna** ou
**Gojo**. Mire num cartão e clique; o cartão fica marcado ("CLIQUE DE NOVO PARA CONFIRMAR") e o
segundo clique, em até 3 segundos, escolhe. A escolha fica gravada no mundo
(`sukuna/progresso.txt`, linha `personagem=`) e o menu não a desfaz. Depois disso, N abre o
menu de sempre.

Antes de escolher, as teclas das técnicas (R, G, V, B) só avisam "Aperte N para escolher o
personagem". Soco, defesa, pulo carregado, barra de energia (M) e técnica reversa (H) valem
para os dois. Mundos de versões antigas também perguntam na primeira vez (escolha o Sukuna
para continuar como antes; nível, status e o resto ficam).

Para trocar depois: **/jujutsu personagem gojo** ou **/jujutsu personagem sukuna**. Sem o
nome, o comando diz qual é o personagem agora. A troca para o que estava em uso e mantém o
resto do progresso (os pontos gastos nos nós da árvore do outro personagem voltam).

## Gojo

As técnicas do Gojo usam as mesmas teclas do Sukuna. Nesta versão existem o **Azul** (com o
Azul V1), o **Vermelho** (com o Vermelho V1), o **Roxo** (com o Roxo V1 e o Roxo 200%) e o
**Mugen** e a expansão de domínio, o **Vazio Ilimitado**. No menu, o painel do meio mostra a
árvore de habilidades do Gojo no lugar da do Sukuna (ver abaixo). Os pactos só do Sukuna não
aparecem para o Gojo, e ele tem os dele (ver Pactos); o "Só os punhos" também proíbe o Azul.
Os tempos de espera das técnicas do Gojo aparecem embaixo da barra azul, como os do Sukuna.

### Árvore do Gojo

Igual à do Sukuna: a mesma maestria (de 1 a 100, um ponto da árvore a cada nível par), as
técnicas acendem quando desbloqueadas e os nós sobem até 5/5, 5% por nível. Usar treina a
maestria: cada Azul dá 4 (o V1, 8), o Vermelho 6 (o V1, 12), o Roxo 15 (o 200%, 30; o V1, 15),
o Vazio 20, e o Mugen ligado dá 2 a cada 5 s.

- **Azul:** desde o início. Nós: raio do puxão, força do puxão, menos energia.
- **Mugen:** maestria 8. Nós: alcance, menos energia.
- **Técnica reversa:** a mesma do Sukuna (maestria 20, três estágios de 2 pontos), no tronco.
- **Vermelho:** sai da técnica reversa: **só desbloqueia com ela aprendida** (pelo menos 1/3).
  Nós: dano, menos recarga, menos energia (o raio fica de fora: o Vermelho continua pequeno).
- **Roxo:** sai do alto do Azul e do alto do Vermelho: **pede o Azul e o Vermelho
  desbloqueados** e maestria 30. Vale para o Roxo, o V1 (sem ele, o Vermelho V1 passa direto
  pelo Azul V1) e o 200%; sem ele, R e V juntos são só o Azul e o Vermelho. Nós: dano, menos
  energia.
- **Vazio Ilimitado:** maestria 45. Nós: raio, duração, menos energia (abrir e manter).

Uma técnica bloqueada avisa o que falta quando a tecla é apertada. Trocar de personagem com
`/jujutsu personagem` devolve os pontos gastos nos nós do outro (a técnica reversa, que é dos
dois, fica).

### Azul (tecla R)

Aperte **R**: uma esfera azul pequena nasce na sua mão (0,35 s) e vai com ela; depois sai
voando (45 blocos/s) para onde você está mirando e **para cerca de 1 bloco antes** do que a
mira encontra (sem nada na mira, a 39 blocos). Lá ela cresce com um tranco (0,45 s, de 0,22
para 1,3 blocos de raio, passando um pouco e voltando), **puxa** por 3 segundos e encolhe até
sumir (0,35 s).

- Puxa mobs, itens e outras entidades num raio de 9 blocos (13,5 com a barra de energia
  cheia), mais forte quanto mais perto; nunca o jogador.
- Quem chega a menos de 1,6 bloco do centro fica preso nele e leva 4 de dano (vezes a força)
  a cada meio segundo.
- Gasta 12 + 3% da energia máxima (vezes a barra da direita, como as outras técnicas);
  recarga de 2 s.
- A esfera é desenhada por um passe próprio: a luz em volta dela é entortada com a lente do
  buraco negro do Orbital Laser (o mundo atrás aparece curvado em volta), e por dentro um
  ruído gira em espiral, mais rápido no meio, com um miolo claro e fios de luz. Na mão ela
  não entorta nada (tão perto dos olhos, entortaria a tela toda); a lente acende no voo.
  Ilumina o terreno em azul.
- Tudo anda pelo relógio, não pelos quadros: a mesma esfera a 30 ou a 240 fps.

### Azul V1 (Shift + R)

Com **Shift** segurado, aperte **R**: uma esfera azul grande (2,2 blocos de raio) cresce na
mira, a 9 blocos dos olhos (ou um pouco antes do que a mira encontra, nunca a menos de 4,5).

- **Enquanto R está segurado** ela segue a mira com atraso, como presa a ela por uma corda
  elástica: vai atrás, passa um pouco do ponto e volta. Dá para segurar até 8 segundos; depois
  disso (ou se uma tela abrir) ela é solta sozinha.
- **Ao soltar R** ela fica parada onde está por **5 segundos** e então encolhe até sumir.
- O tempo todo **puxa** entidades num raio de 13 blocos (mais com a barra de energia), mais
  forte que o Azul; quem entra no corpo dela fica girando perto do centro e leva 6 de dano
  (vezes a força) a cada meio segundo.
- **Arranca blocos** num raio de 4,8 blocos do centro, os mais perto primeiro, 4 a cada dois
  ticks, até 160 por esfera: cada um sai como um bloco caindo, que ela puxa e segura. Quando
  ela some, eles caem e se assentam onde pararem (ou viram item, se não couberem). Nunca tira o
  chão perto do jogador, nem bedrock, água, lava, portais e afins. Com "Destruição de blocos: NÃO"
  ela não arranca nada.
- **Destrói o que já absorveu**: um bloco arrancado que chega ao corpo da esfera some (não vira
  item). Assim, segurando o Azul V1 parado num lugar, ele vai comendo o terreno em volta.
- Gasta 30 + 7% da energia máxima (vezes a barra da direita), uma vez, ao criar; recarga de
  6 s; só um no ar por vez. A recarga é separada da do Azul normal.

### Vermelho (tecla V)

Segure **V**: a carga tem três estágios.

1. Até 0,5 s: uma esfera vermelha pequena se forma na mão.
2. De 0,5 a 1,5 s: ela cresce e aparecem raios vermelhos e faíscas em volta.
3. De 1,5 a 2 s: raios e faíscas no máximo; cheia, ela pulsa.

Cada estágio tem o seu som. **Solte V**: uma explosão vermelha **em volta de você**, sem fogo,
que **empurra tudo para longe** (sempre um pouco para cima) e
fere quem está perto, com uma onda de choque: uma casca vermelha que cresce, acende uma faixa
onde corta o terreno e entorta a imagem por onde passa, mais um clarão no começo e um tranco na
câmera. Com menos de 0,5 s, nada sai e nada se gasta. Segurado cheio por mais de 10 s, dispara
sozinho.

- O foco é **dano numa área pequena**: o raio vai de 2 blocos (barra da direita vazia) a **5
  blocos** (barra cheia) e nunca passa disso. A carga (25% a 100%) e a barra aumentam o dano, de
  19 a 80 (vezes a força do jogador, menos na borda), e o empurrão. **Destrói blocos em volta de
  você**: uma cratera de cerca de 1,6 a 4,5 blocos de raio (cresce com a carga e com a barra; um bloco a
  cada três voa para fora). O chão embaixo dos seus pés fica; com "Destruição de blocos: NÃO",
  sem cratera.
- Gasta 18 + 4% da energia máxima (vezes a barra da direita) ao disparar; recarga de 3 s.

### Vermelho V1 (V cheio + botão direito)

Com o Vermelho cheio (2 s) e V ainda segurado, **segure o botão direito**: a esfera vai para o
meio, na frente dos olhos, e a visão fecha um pouco (zoom), para mirar. Soltar V nessa hora não
dispara. **Solte o botão direito**: a esfera é lançada a 80 blocos/s para onde você mira e
explode onde bater (num mob ou um pouco antes do bloco), na força toda e maior que a do
Vermelho comum: empurra até 12 blocos, fere até 5, com até 30 de dano, e abre uma cratera.
Antes de encher, o botão direito só avisa. O gasto e a recarga são os do Vermelho.

### Roxo (R + V juntas)

Aperte **R e V juntas** (uma logo depois da outra, em até 0,25 s), **uma vez só**. O que as
teclas começaram (o Azul, a carga do Vermelho) se desfaz, com a energia devolvida, e uma esfera
azul e uma vermelha surgem dos lados, na frente, e se juntam (0,45 s) numa roxa, com um clarão.

- **Enquanto R ou V estiver segurada**, ela carrega: a barra roxa embaixo da mira enche em
  2,5 s, e a esfera cresce, com faíscas. **A esfera que você vê na frente é do tamanho da que
  vai sair**: o que cresce na carga é o que é lançado.
- **Soltando as duas**, ela é lançada para onde você mira. Solta cedo, é o **Roxo compacto**
  (apaga 0,8 bloco de raio, vai 40 blocos); cheia, o **Roxo clássico** (2,6 blocos de raio, 100
  blocos, mais rápido e mais forte). No meio, no meio.
- Ele **apaga** o que estiver no caminho: os blocos somem sem deixar item (nunca o chão do
  jogador, nem bedrock, água, lava e portais; com "Destruição de blocos: NÃO", nenhum) e cada mob
  que ele atravessa leva 20 a 80 de dano (vezes a força), uma vez. No fim, some com um estalo
  roxo.
- Gasta (25 + 6% da energia máxima) vezes 1 a 2 com a carga, vezes a barra da direita, ao
  lançar; recarga de 4 s. Precisa ter a energia do compacto para começar.

### Roxo V1 (Vermelho V1 num Azul V1)

Deixe um **Azul V1 parado** (solto) e lance um **Vermelho V1** através dele: os dois se fundem
numa esfera roxa instável no lugar do Azul ("Roxo instável: 6 segundos, saia de perto!"), com
um tranco leve na câmera. Ela **cresce até ficar 50% maior**, chegando ao tamanho todo em **4
segundos**; daí em diante **solta raios roxos** e pulsa cada vez mais rápido, e em **6 segundos**
explode. A explosão é
**catastrófica**, sem fogo, e **quanto mais cheia a barra da direita (M) na hora da fusão, mais
forte**: de 3 a 7 vezes a do Vermelho V1.

| Barra (M) | Empurra até | Fere até | Dano nos mobs | Cratera (raio) | Dano em você |
|---|---|---|---|---|---|
| vazia | 36 blocos | 15 blocos | 270 | 6 blocos | até 24 |
| meia | 60 blocos | 25 blocos | 750 | 10 blocos | até 42 |
| cheia | 84 blocos | 35 blocos | 1470 | 14 blocos | até 60 |

O dano nos mobs é vezes a força do jogador, menos na borda. **A explosão destrói blocos** (a
cratera da tabela; com "Destruição de blocos: NÃO", nenhum). **Fere o jogador também**, se você
estiver dentro do alcance de ferir (com a barra cheia, 60 de dano mata sem armadura). A
cratera arranca tudo no raio (até 200 blocos voam para fora); abrir uma de 14 blocos pode
travar o jogo um instante.

### Roxo 200% (Shift + o gesto do Roxo)

Com **Shift** segurado, faça o gesto do Roxo (R + V juntas, uma vez). Uma cena de pouco mais de 3 s:

1. Uma esfera azul e uma vermelha grandes surgem atrás de você, acima dos ombros (1 s).
2. Elas passam por cima dos seus ombros até a frente (1 s).
3. Se fundem numa esfera roxa enorme (4 blocos de raio) na sua frente, com um tranco na câmera.
4. Ela sai para onde você estiver mirando, apagando um túnel de 4 blocos de raio por 150 blocos
   (200 de dano em cada mob no caminho), e explode no fim com uma onda de choque roxa grande
   (sem fogo; empurra até 30 blocos e abre uma cratera).

Você pode andar e virar durante a cena: as esferas vão com você. Gasta 100 + 30% da energia
máxima (vezes a barra da direita) ao começar; recarga de 30 s. O "Só os punhos" proíbe o Roxo
também.

### Mugen (tecla G)

**G** liga e desliga o Infinito ("Mugen: ligado"; "MUGEN" embaixo da mira). Ligado:

- **O tempo de tudo o que se aproxima passa mais devagar quanto mais perto chega**, num alcance
  de 3 blocos (barra da direita vazia) a 5 (cheia) do meio do seu corpo. Na borda, normal; a meio
  caminho, metade; e na distância em que encostaria, parado: a velocidade do tempo é a proporção
  do que falta.
- **Mobs** (e carrinhos e barcos): só andam nos ticks em que o tempo deles juntou um tick
  inteiro. A meio caminho, um tick sim, outro não; perto, quase nunca. Tudo deles fica lento
  junto: o passo, o virar da cabeça e a espera entre um ataque e outro. A parada de um mob é
  antes de o golpe dele te alcançar (0,3 + meia largura dele + 0,83 + 0,2 de folga; um zumbi para
  a 1,63 bloco), então **com você parado ele nunca chega**.
- **Projéteis** (flechas, tridentes, bolas de fogo, blocos caindo...): perdem a velocidade do
  mesmo jeito e ficam pendurados no ar antes de encostar. Mesmo uma flecha rápida, vinda de fora,
  é segurada na borda.
- Se **você** anda para cima de algo, ele é posto de volta na parada (para o lado, na mesma altura).
- **Nenhum golpe com alguém por trás chega**: socos de mob, flechas, tridentes, até a explosão
  de um creeper. O que não tem ninguém por trás (queda, fogo, lava, afogamento) chega normal.
- Itens e experiência não são parados, para você poder pegá-los.
- **Não há esfera à vista: só o espaço se entortando.** O que está dentro do alcance tremula
  como através de vidro, com ondas finas andando para fora e as cores se separando um pouco; e
  em volta de cada coisa que chega perto o espaço se enruga, puxado para o meio e ondulando,
  mais forte quanto mais perto ela está.
- Gasta 3 + 1% da energia máxima por segundo (vezes a barra da direita). Sem energia, ou com o
  pacto "Só os punhos", desliga sozinho.

### Vazio Ilimitado (tecla B)

**Segure B por 1,5 s** (uma barra "VAZIO" enche embaixo da mira; levar dano quebra a
concentração, e soltar antes não abre nada). O domínio fechado nasce dos seus pés: uma esfera
de raio 20 (barra da direita vazia) a 125 blocos (cheia) cresce em 1,6 s, com uma película
colorida na parede, e quando se fecha há um clarão branco. Aí o mundo some:

- **A barreira é preta por fora**: de fora, o domínio é uma esfera negra, só com um brilho fraco
  na borda. **De dentro não se vê nada do mundo de fora**, nem os blocos além da parede.
- **Você não sai da barreira**: chegando perto da parede, você é empurrado de volta para
  dentro.

- **Dentro é o vazio**: um espaço sem fim, escuro, com estrelas, nuvens de luz azul e roxa,
  fios de informação correndo e, para onde você olhava ao abrir, um **buraco negro enorme**:
  a sombra preta, o anel fino em cores na borda e o gás branco-azulado girando em espiral em
  volta (a lente é a do buraco negro do Orbital Laser, a mesma das esferas).
- **O chão é uma lâmina de água transparente** sobre o vazio: reflete o buraco negro e o céu
  (mais quanto mais rasante você olha) e deixa ver o vazio embaixo. Quem anda nela (você ou
  qualquer entidade) deixa ondulações que se abrem e somem em 3 s. Embaixo da água há um piso
  invisível de barreira; tudo o que está acima dele dentro da esfera some e volta no fim,
  como no domínio fechado do Sukuna (blocos, baús com o que têm, quadros, fluidos).
- **Tudo o que a esfera alcança fica paralisado**: os mobs perdem o cérebro (`NoAI`): não
  andam, não atacam e nem viram a cabeça; o que não é vivo (itens, barcos, carrinhos) para no
  ar (`NoGravity`). **Projéteis não param** (flechas, tridentes, bolas de neve, bolas de fogo,
  cargas de vento, poções jogadas...), nem você. Quem entra depois também é pego. No fim,
  cada um volta exatamente como estava (um mob que já era `NoAI` continua sendo), mas só **2
  segundos depois** de o domínio fechar: nesse tempo os mobs continuam paralisados.
- Dura 15 s; **B de novo** fecha antes. A esfera encolhe de volta até você em 1,2 s e o
  mundo volta. Abrir gasta 60 + 12% da energia máxima, manter gasta como o domínio do Sukuna
  (6 + 0,75% da máxima por segundo), tudo vezes a barra da direita; sem energia, fecha
  sozinho. Recarga de 30 s. O pacto "Sem expansão de domínio" (e o "Só os punhos") proíbe.
- Trocar de dimensão fecha o Vazio; fechar o mundo solta todo mundo e devolve os blocos antes
  de salvar.

## Como usar no jogo

1. Segure **R** (dá para trocar em Opções > Mods > Jujutsu Horizon).
2. Com R segurado, segure o **botão esquerdo do mouse** e mova a mira: sai um traço vermelho no ar.
   Solte o botão e segure de novo para fazer outro traço (até 8).
3. Solte **R**: cada traço vira um corte.

O corte parte dos seus olhos e passa exatamente por trás do traço na tela, então o que você
riscou por cima de um mob ou de uma parede é o que é cortado, a qualquer distância. Quanto mais
longe, mais largo o corte fica.

Enquanto R está segurado, o botão esquerdo não bate nem quebra blocos.

A dica de como desenhar aparece na barra de ação só nas **3 primeiras vezes** que você aperta
R (a contagem é guardada nas opções do mod); depois, não aparece mais.

**No ar:** se você apertar R fora do chão, passa a cair devagar enquanto desenha, mas tem no
máximo **6 segundos**: a contagem aparece na barra de ação e, quando acaba, os cortes saem
sozinhos (como se você tivesse soltado R).

## Animações

Enquanto uma técnica está em uso, o corpo do jogador se move com ela, com a pele dele:

- **Desmantelar:** a mão direita estendida para onde o traço está sendo desenhado; ao soltar,
  um empurrão para a frente.
- **Cleave:** a mesma mão estendida, agarrando o mob preso, tremendo de força.
- **Corte de teia:** o pouso de herói: o corpo desce enquanto a teia carrega, com um joelho
  no chão, e a mão direita bate espalmada no chão quando o corte sai. Em primeira pessoa a
  câmera desce e olha para o chão junto.
- **Flecha de fogo:** as mãos juntas onde o fogo se forma, depois o corpo de lado, o braço
  esquerdo esticado à frente e a mão direita puxando a flecha até o rosto.
- **Expansão de domínio:** as mãos juntas diante do peito no selo, tremendo conforme a
  concentração dura, e os braços que se abrem quando o domínio se expande.
- **Redemoinho do Cleave:** os dois braços para cima e abertos, em Y, com os cotovelos um
  pouco dobrados, tremendo de leve enquanto G fica segurado.

Do Gojo:

- **Azul e Vermelho:** a mão direita na frente do rosto, com a palma para cima, e o poder
  se formando e flutuando acima dela. Ao lançar, a mão sai dessa pose para a frente, como o
  empurrão do Desmantelar.
- **Azul V1:** enquanto R fica segurado, a pose clássica: a mão direita erguida ao lado do
  rosto, com o cotovelo para baixo e para fora.
- **Roxo:** os braços se abrem para os lados, onde nascem o Azul e o Vermelho, e se fecham na
  frente quando os dois se juntam; o lançamento é o mesmo empurrão para a frente.

Em terceira pessoa o mod desenha o corpo inteiro com a pele do jogador (largo ou fino); nesse
tempo a armadura, o item na mão e a capa não aparecem. Em primeira pessoa a mão do jogo some
e entram os dois braços do mod, nos cantos de baixo da tela, como os do jogo. Sem técnica em
uso, tudo é como no jogo.

## Combate de mão vazia

Tudo isto vale com a **mão principal vazia**; com qualquer item na mão, os botões do mouse são
do jogo, como sempre.

- **Soco:** segure o **botão esquerdo** para carregar e solte para socar. Não há espera entre
  um soco e outro. O soco normal pede **0,5 s** de carga (6 de dano); menos que isso dá pouco
  dano (1 a 4), mas dá para emendar socos rápidos num **combo**: direto de direita, cruzado
  de esquerda, gancho e uppercut, e de novo (um soco até 0,7 s depois do outro continua o
  combo). A carga vai até **1 s** (12 de dano). A Força multiplica, como em toda habilidade.
- Todo soco que acerta **desnorteia** o mob por 0,15 s (ele para) e o empurra; o empurrão
  cresce com a carga.
- **Soco carregado até o fim:** se o dano (já com a Força) passar de 10, solta uma **onda de
  choque pequena**, rápida e quase transparente (some em 1 s), no ponto do soco, que empurra
  e fere (um terço do dano) quem estiver a até 3,5 blocos.
- Depois de cada soco o corpo fica **na guarda por 1 segundo**; sem bater nesse tempo, volta
  à postura normal.
- Mirando num bloco ao alcance, sem mob na frente, o botão esquerdo quebra o bloco como
  sempre.
- **Defesa:** **Shift + botão direito** segurados: os antebraços na frente do rosto, e todo
  golpe chega com **40% a menos**. Solte qualquer um dos dois e a defesa acaba.
- **Aparo de projétil:** clique com uma flecha (ou bola de fogo, tridente, bola de neve...)
  vindo na sua direção, à frente e a até 4,5 blocos: um tapa de costas da mão e ela volta
  **para onde você mira**, mais rápida do que veio.
- **Aparo de mob:** se o seu soco acerta o mob até **0,2 s antes do golpe dele**, o golpe não
  sai e ele fica **desnorteado por 1,5 s** (parado, sem atacar, com estrelinhas em volta da
  cabeça).

### Black Flash

Logo depois de um soco **acertar**, aperte **os dois botões do mouse juntos** (esquerdo e
direito, com no máximo 0,08 s entre eles) numa janela curtinha de **0,25 s** a partir do
impacto: o golpe vira um **Black Flash**. Os dois botões têm que descer depois do soco acertar.

- O mob leva mais uma vez e meia o dano do soco (pelo menos 6), é empurrado e fica
  desnorteado por 0,5 s; tranco na câmera, estrondo e "BLACK FLASH" na tela.
- **No chão**, o impacto solta raios vermelhos e escuros, curtos e juntos, e um clarão em volta.
- **No ar**, os raios saem **negros e mais espalhados**, até 4,5 blocos; cada outro mob a até 4,5
  blocos do impacto leva 1 de dano (fora o que levou o soco).
- A janela é a mesma com qualquer soco que acerta (carregado, rápido ou de combo).

As animações (guarda, carga, golpe, defesa e tapa) aparecem em terceira pessoa com o corpo
inteiro e em primeira pessoa com os braços.

## Pactos vinculativos (painel PACTOS do menu, o último à direita)

Aceite uma restrição por um tempo e ganhe dano em troca, como o pacto que deixa o domínio
aberto mais forte por abrir mão da barreira:

| Pacto | Quem | Restrição | Recompensa |
|---|---|---|---|
| Sem expansão de domínio | os dois | não abrir o domínio por 10 minutos | +30% de dano |
| Sem técnica reversa | os dois | não se curar pela reversa por 10 minutos | +25% de dano |
| Só os punhos | os dois | nenhuma técnica, nem a reversa, por 5 minutos | +60% de dano |
| Corpo como preço | os dois | metade da vida máxima por 5 minutos | +35% de dano |
| Sem Flecha de Fogo | Sukuna | não usar a Flecha de Fogo por 10 minutos | +20% de dano |
| Sem Cleave | Sukuna | não usar o Cleave (nem o Corte de teia) por 10 minutos | +15% de dano |
| Sem Vazio Roxo | Gojo | nenhum Roxo (nem o V1, nem o 200%) por 10 minutos | +25% de dano |
| Sem Infinito | Gojo | não ligar o Mugen por 10 minutos | +15% de dano |

O painel só mostra os pactos do personagem que você está usando.

- Clique em **ACEITAR** e depois em **CONFIRMAR?** (em até 3 segundos): o pacto está selado.
- Um pacto **não se quebra**: enquanto dura, a técnica proibida não sai e o aviso mostra quanto
  falta. O tempo só corre com o jogo rodando e fica gravado com o mundo.
- Os bônus de vários pactos ao mesmo tempo **se somam** e valem para todo dano do mod
  (técnicas e socos), junto com a Força.
- Com o domínio aberto, o pacto do domínio não pode ser aceito: feche-o antes.

## Feridas

Todo mob cortado fica com a ferida no corpo, no lugar e no ângulo em que o corte passou: a
borda da pele, o músculo e, nos trechos mais fundos, o osso, com sangue escorrendo para baixo
por alguns segundos. A ferida anda e vira junto com o mob e fica até ele morrer ou sumir de
vista. Cada mob mostra as oito feridas mais recentes, e até 8 mobs feridos aparecem ao
mesmo tempo. No golpe, o corpo do mob pisca em vermelho.

## Cleave

1. Mire num mob a até 5 blocos e **segure G** (tecla trocável em Opções > Mods > Jujutsu Horizon).
2. O mob é preso diante de você, com o peito na mira, e segue a mira enquanto G estiver segurado.
3. Ele leva um corte pequeno a cada 3 ticks, cada vez mais fundo: de arranhões a osso em uns
   doze cortes. O corpo mostra os oito cortes mais recentes.
4. O dano entra duas vezes por segundo: 4 com a barra vazia, até 16 com ela cheia.
5. **Solte G** para largar o mob, que leva um empurrão para a frente.

A mão segura no máximo **5 segundos**: depois solta sozinha, com o empurrão, e a recarga
começa. Também solta se o mob morrer, a energia acabar ou uma tela abrir. Num segurar inteiro
são 10 golpes: 40 de dano com a barra vazia (um zumbi morre em uns 2,5 s) e 160 com ela
cheia, antes da Força e da árvore.

### Redemoinho (G olhando para cima)

Aperte e segure **G olhando para cima** (mais de 50° acima do horizonte): em vez de agarrar,
os cortes do domínio aparecem numa esfera em volta de você, de **3 blocos de raio** com a
barra vazia até **18 blocos** com ela cheia. Todo mob dentro leva dano moderado duas vezes por
segundo (7 com a barra vazia, até 21 com ela cheia, antes da Força) e fica com as feridas no
corpo. Dura enquanto G fica segurado, até **4 segundos**; gasta a energia do Cleave
segurando e, ao acabar, começa a recarga do Cleave. Não mexe no terreno.

## Barra de energia

**Segure M** para encher a barra, que aparece acima dos corações. Ela vai de 0% a 100% em 3
segundos e fica onde você a deixar. **Shift + M** esvazia. Cada habilidade usa o nível da
barra no momento em que é usada:

| Habilidade | Com a barra vazia | Com a barra cheia |
| --- | --- | --- |
| Desmantelar | 48 blocos de alcance, 14 de dano | 3x o alcance, 4x o dano, 2x a velocidade, corte mais grosso |
| Cleave | um corte a cada 3 ticks | um corte por tick, 4x o dano, osso à mostra em três cortes |
| Flecha de fogo | explosão de 5 a 9 blocos de raio | explosão nuclear (abaixo) |

## Flecha de fogo

1. **Segure V** (tecla trocável em Opções > Mods > Jujutsu Horizon): o fogo se junta entre as mãos,
   em espiral, e vira uma flecha que se forma de trás até a ponta enquanto a mão direita a
   puxa até o rosto. A carga enche em 1,5 segundo;
   menos de 0,35 segundo não dispara.
2. **Solte V**: a flecha voa reta até o primeiro bloco ou mob na mira e explode.

A explosão assenta no chão: se a flecha acerta um mob ou explode até alguns blocos acima do
terreno, a cratera e o cogumelo saem do chão embaixo, não do ar. Só explode no ar de verdade
quando o alvo está alto.

**Abaixo de 74% da barra não há cogumelo:** só o clarão, a onda de choque (uma casca que se
espalha do ponto do estouro e um anel de poeira correndo pelo chão) e a cratera. Quanto mais
perto de 0%, mais fraca e menor ela é; some em 3,5 segundos.

**Dos 74% em diante** a explosão na tela é a da Nuke do mod Orbital Laser, desenhada pelo
shader dele (licença MIT, veja o arquivo `NOTICE`): o clarão, a bola de fogo, o cogumelo, a
onda de choque, a poeira e os destroços. Ela começa no instante do estouro, sem a bomba
caindo do céu, e é encolhida para o tamanho da explosão: com a barra cheia e a carga cheia é
a Nuke em tamanho real. O cogumelo fica no céu por uns **20 segundos** (apaga nos 3 finais).

Sem energia na barra, o raio vai de 5 a 9 blocos conforme a carga. O terreno é cavado do meio para fora, e o chão da cratera vira blackstone, magma e basalto.
Mobs em volta queimam e são lançados para fora e para cima.

O tamanho cresce com o quadrado da barra: a 50% a cratera tem uns 32 blocos de raio; a 100%,
com a carga cheia, é a escala da bomba nuclear do Orbital Laser:

- cratera de 200 blocos de lado a lado e 40 de fundo, cavada em 3 segundos;
- tudo o que está de pé sobre a cratera some junto, até 60 blocos acima do chão;
- a mesma imagem da Nuke do Orbital Laser, sem escala; a fumaça some em 20 segundos (apaga
  dos 17 aos 20);
- mobs a até 125 blocos (um quarto além da borda da cratera) são atingidos, com dano de
  dezenas de vezes o de um corte no meio.

Em qualquer tamanho, o dano vai **só até um quarto além da borda da cratera** e cai com o
quadrado da distância: forte no meio, quase nada perto da borda, zero depois dela.

Você não toma dano da própria explosão, mas a 100% o chão num raio de 100 blocos some: atire
de longe. Crateras grandes não avisam os blocos vizinhos (seriam atualizações demais), então
areia e água na borda ficam paradas até algo mexer nelas. Nada é cavado nos cinco blocos mais
fundos do mundo, onde fica o bedrock.

Com "Destruição de blocos" em NÃO, a explosão não cava cratera.

O shader da Nuke é grande e é montado em segundo plano na primeira vez que o mod desenha
algo. Enquanto ele não está pronto (alguns segundos), ou se a placa não puder desenhá-lo
(OpenGL, ou o mundo desenhado em resolução menor que a janela), a explosão usa o shader
antigo do mod (`blast.wgsl`). Só as duas explosões mais novas usam o da Nuke ao mesmo tempo.

## Expansão de domínio (tecla B)

Segure **B** sem levar dano: qualquer dano no jogador quebra a concentração. Uma barra
aparece abaixo da mira, com uma marca nos 3 segundos.

- **3 segundos e soltar: domínio fechado.** Uma barreira em esfera perfeita se fecha em volta
  de você (desenhada por shader, sem blocos): negra por fora, uma abóbada escura por dentro.
  O raio vai de 20 blocos sem energia a 125 com a barra cheia (250 de diâmetro).
- **5 segundos: domínio aberto.** Abre sozinho, sem barreira e com o dobro do alcance do
  fechado: de 40 a 250 blocos de raio. Em troca, quem está dentro pode sair andando. Um
  círculo em brasa no chão marca até onde ele vai.

Dentro do domínio, por 15 segundos:

- todo mob apanha de cortes sem parar (o dano das opções, até o triplo com a barra cheia),
  e os mais próximos ficam com as feridas no corpo;
- no domínio **aberto**, cortes finos caem no terreno em direções sorteadas e fatiam o que
  está de pé (árvores, construções, morros) do chão dos seus pés para cima, ao longo de
  linhas: o resto fica, e não volta. Só o chão do santuário (e em volta de você) é limpo de
  todo, para ele caber. Com "Destruição de blocos" em NÃO, o terreno fica;
- no domínio **fechado**, só sobram o piso e o santuário: o chão dentro da barreira (a camada
  logo abaixo dos seus pés) vira um piso preto com uma lâmina de água rasa por cima, que
  reflete a abóbada em brasa, e **tudo o que está acima do piso, dentro da barreira, some**
  (árvores, casas, morros, água, lava, baús). O piso se espalha do meio para fora junto com a
  barreira, e logo depois o resto some, um chunk por vez, do meio para fora (lido de uma cópia
  do chunk, sem perguntar ao jogo bloco a bloco; num domínio grande numa montanha leva alguns
  ticks a mais, para o jogo não travar). Também vale no ar: aberto no alto, o domínio tem
  chão. Nada voa nem cai: some. Antes de trocar ou tirar cada bloco, o mod guarda o que havia
  ali (com o que estiver dentro de um baú, por exemplo). **Quadros, molduras (com o item) e
  nós de laço** dentro da barreira saem antes da parede deles sumir, guardados com tudo o que
  têm, sem cair como item. **Água e lava que escorrerem para dentro** enquanto o domínio dura
  são tiradas a cada meio segundo, e mais uma vez logo antes de tudo voltar. **Quando o domínio
  acaba, tudo volta exatamente como era**, como se nunca tivesse sumido (os quadros no lugar),
  quase sempre num tick só. Se o mundo fechar com o domínio aberto, tudo volta antes de o mundo
  ser salvo. Isso não depende de "Destruição de blocos" (nada se perde). Bedrock, portais e blocos
  de comando ficam como estão. A água do piso é só desenho: não molha nem escorre. Vale para o
  domínio fechado de qualquer personagem (o do Gojo, quando vier, usa o mesmo);
- o santuário nasce logo atrás de você, virado para onde você olhava, e é o meio do domínio
  (a barreira e o alcance são medidos a partir dele, não de você): o modelo "Malevolent Shrine |
  Jujutsu Kaisen", de NexusB (CC BY 4.0, veja o `NOTICE`), com 13 blocos de altura.

Apertar **B** de novo termina o domínio antes da hora.

### A barreira

- Prende os mobs do lado em que estão e empurra você de volta quando encosta nela.
- Para cortes e flechas de fogo: a flecha explode do lado de onde veio, e a cratera e o dano
  dela ficam só desse lado.
- Tem vida, mostrada no topo da tela, e racha conforme apanha. Uma flecha de fogo com a
  barra e a carga cheias tira 100 de 350 quando vem de fora: três não quebram, a quarta sim.
  De dentro, cada golpe vale um terço: são onze flechas dessas. Um corte do Desmantelar
  tira de 2 a 8.
- Quebrada, o domínio acaba.

O que conta como golpe na barreira são as habilidades deste mod; TNT, creepers e outros mods
não a atingem.

## Aura de energia

Com a barra de poder (M) acima de 5%, o ar em volta do corpo **tremula**, como sobre o fogo:
ondinhas finas que sobem e acompanham a silhueta, **pretas e da cor da energia**: azul com o Gojo,
vermelha com o Sukuna. Entre 5% e 15% ela aparece aos poucos; depois fica mais forte e mais
larga conforme a barra enche. Acende e apaga devagar. Em primeira pessoa, olhando em frente, a
tela não treme (só olhando para os pés). Não há mais fumaça.

Enquanto a **técnica reversa** está sendo usada (H segurado fora da recarga, ou curando no
automático), as ondinhas ficam **pretas e verdes**. A força vem do estágio da reversa: no 1 é
como a aura da barra em 33,33%, no 2 em 66,66% e no 3 em 99,99%. Se a barra já fazia uma aura
mais forte, ela continua desse tamanho, só que verde enquanto a reversa durar.

## Comandos

Todos aceitam **/jujutsu** ou **/sukuna**.

**/jujutsu personagem <sukuna|gojo>**: troca o personagem do mundo (ver a escolha no topo).

## Comando /jujutsu max

Digite **/jujutsu max** no chat: nível 500, os cinco status em 500, maestria 100, todos os nós
da árvore no máximo (todas as técnicas liberadas, inclusive o corte do mundo), energia cheia,
barra de poder cheia, recargas zeradas e **pontos de loja sem limite** (o painel mostra "SEM
LIMITE" e as compras não descontam nada). Fica gravado no mundo, como o progresso normal. Não
tem volta pelo comando: para recomeçar, apague `sukuna/progresso.txt` dentro da pasta do
mundo com o jogo fechado.

## Menu do sistema (tecla N)

O menu abre em painéis holográficos que flutuam em arco na sua frente, na altura dos olhos:

- **à esquerda**, o **STATUS** (nível, experiência, pontos, os cinco status, cada um com o seu
  **+**, e embaixo a maestria e os pontos da árvore e da loja), da mesma altura da árvore e um
  pouco mais largo; e **mais à esquerda dele**, a **CONFIG**;
- **no meio**, a **ÁRVORE DE HABILIDADES**;
- **à direita**, os **PORTÕES** e, depois deles, a **LOJA**, do mesmo tamanho. As duas listas
  rolam: mire nelas e use a **roda do mouse** (ela não troca o item da mão enquanto a mira
  estiver num painel). Depois da loja, os **PACTOS** (abaixo).

A mira continua livre: você vira a câmera normalmente e usa a **própria mira do jogo** para
apontar. O que estiver na mira acende, e um clique (esquerdo ou direito) usa. **Shift +
clique** num **+** de status coloca 10 pontos de uma vez. Clicar fora dos painéis bate e
quebra blocos como sempre.

Os painéis abrem e fecham com uma animação curta (se desdobram de uma linha), ficam virados
para onde você olhava quando abriu e sempre por cima do mundo (uma parede perto não os
esconde). **Andar** (sair do lugar onde abriu) ou apertar de novo a tecla do menu fecha.

## Loja (painel da direita do menu, depois dos Portões)

Itens do jogo pagos com **pontos de loja**, que só vêm de Portões vencidos: 2 (rank E), 4
(D), 7 (C), 12 (B), 20 (A) ou 35 (S), o dobro nos vermelhos de chefe. Os preços são altos de
propósito:

| Item | Preço |
|---|---|
| Garrafas de experiência x16 | 10 |
| Poção de cura forte x2 | 14 |
| Pérolas do End x4 | 16 |
| Maçãs douradas x2 | 22 |
| Diamante | 30 |
| Totem da imortalidade | 75 |
| Livro encantado: Remendo | 90 |
| Lingote de netherite | 150 |
| Maçã dourada encantada | 180 |
| Bloco de diamante | 240 |
| Élitros | 320 |
| Estrela do Nether | 400 |

Clique em **COMPRAR**: os pontos saem na hora e o item chega no inventário (com o inventário
cheio, cai no chão, como no `/give`). Se o servidor não conseguir entregar, os pontos voltam.

## Sistema: nível, status e energia (tecla N)

O progresso é guardado por mundo (`sukuna/progresso.txt` na pasta do mundo).

- **Experiência:** matar um mob dá a vida dele dividida por 2, mais o dano dele (um zumbi
  dá 13). O primeiro nível pede 100 de XP e cada um pede mais que o anterior; o máximo é 500.
- **Pontos:** cada nível dá 3. Abra o menu com **N** e clique em **+** no painel do status
  (com Shift, dez de uma vez). Cada status começa em 1 e vai até 500:
  - **Força:** multiplica o dano das habilidades (até 6 vezes) e soma ao dano da mão.
  - **Resistência:** reduz o dano recebido (até sobrar um quarto).
  - **Velocidade:** até 40% a mais andando; correndo 5 segundos sem parar, até 60% sobre
    isso. O pulo normal continua igual. **Segurar Shift no chão** enche a barra do pulo
    (abaixo da mira) em 1 segundo; pulando com ela carregada, o pulo leva a carga: até 45
    blocos no status 500 com a barra cheia. Solto o Shift, a carga ainda vale meio segundo.
  - **Energia amaldiçoada:** a energia máxima (100, mais 10 por ponto) e o limitador da
    barra de poder: o status dividido por 5, em por cento. No começo o limitador é 0.
  - **Vida:** um coração a mais a cada 5 pontos.
- **Queda:** do nível 25 em diante, cair não fere.

### As barras

- **Direita, vertical:** a barra de poder (tecla M). Enche devagar (6 s do zero ao máximo),
  esvazia depressa com Shift (1,5 s) e para no limitador, marcado com um traço.
- **Esquerda, vertical:** a energia amaldiçoada, com o valor, o nível e as recargas que
  ainda correm embaixo. Ela volta sozinha: 2 por segundo, mais 1 a cada 25 pontos do status.

### Gastos e recargas

Cada gasto tem uma parte fixa e uma **parte da energia máxima**: assim as técnicas continuam
pesando quando o status de Energia sobe. O gasto da tabela vale para a barra de poder até
25%; a 50% é o dobro, a 100% o quádruplo.

| Habilidade | Gasto | Recarga |
|---|---|---|
| Desmantelar | 3 + 0,4% da energia máxima por disparo | 0,75 s |
| Cleave (e redemoinho) | 8 + 1% por segundo segurando | 12 s |
| Corte de teia | 25 + 4% | 12 s (a mesma do Cleave) |
| Flecha de fogo | 50 + 10% | 40 s |
| Expansão de domínio | 60 + 12% para abrir, 6 + 0,75% por segundo aberta | 90 s depois que acaba |
| Corte do mundo | 80 + 20% | 180 s |

Com a Energia no máximo (5090) e a barra cheia: a flecha leva perto de 44% da energia, a
expansão 53% só para abrir e o resto nos 15 segundos aberta, e cada Desmantelar quase 2%.

### Corte de teia

Com a tecla do Cleave (**G**), sem mob na mão e mirando no chão a até 8 blocos, segure por
1 segundo: uma teia de cortes se abre pelo chão em volta do ponto, fere todo mob em cima dela
(o dobro do dano das opções) e, com "Destruição de blocos" em SIM, rasga o chão. O raio é de 12
blocos, o dobro com a barra de poder cheia.

O dragão e os cristais do End são atingidos por comando em volta de cada golpe, porque o
jogo não os entrega aos mods junto com os outros mobs.

## Portões (painel da direita do menu)

Missões contra mobs do próprio jogo, mais fortes, equipados e, em muitos casos, capazes de
investir contra você: um mob que fica a 5 blocos ou mais por 3 segundos é lançado na sua
direção.

- Escolha um Portão no menu e clique em **ENTRAR**. A missão acontece ali mesmo, no chão: uma
  barreira igual à do domínio, só que **preta e amarela**, sobe em volta de você — uma
  esfera de **50 blocos de raio** com o centro nos seus pés.
- Os mobs vêm em ondas, no chão, entre 10 e 26 blocos de você; a onda seguinte chega quando
  não sobra nenhum mob da anterior.
- Enquanto a missão dura, **nenhum mob nasce sozinho dentro da barreira**, e **nenhuma
  entidade que está dentro sai** (quem encosta na casca por dentro volta um passo). Os mobs
  que já estavam lá continuam lá.
- **Você pode sair, mas 3 segundos fora da barreira encerram a missão** (o aviso aparece na
  barra de ação com a contagem). Voltar antes zera a contagem.
- **Vencer** dá a experiência, os pontos de loja e uns poucos minérios do Portão (bem menos
  que antes: a Cova rasa dá 4 carvões e 1 ferro), e a barreira se desfaz em cacos.
- **Morrer, ficar fora ou abandonar** (botão SAIR no mesmo painel) encerra a missão e fecha
  aquele Portão por 30 segundos.
- **Os mobs da missão brilham em laranja** (o contorno aparece através das paredes, só para
  você), para não se perderem.
- **Os mobs da missão não deixam nada cair**: nem o saque, nem o que vestem, nem experiência.
- **Os mobs da missão sempre caçam você**: ao chegar levam um arranhão seu (até os neutros,
  como endermen e aranhas de dia, ficam com raiva), não largam você nem quando você some de
  vista ou outro mob os acerta, e a cada meio segundo são mandados atrás de você, vendo-o ou
  não, de qualquer ponto da barreira.
- **Os esqueletos vêm com arco** (e os outros com a arma deles: espadas, machados, besta),
  já que o jogo não equipa mobs criados por comando.
- **Durante a missão as habilidades não destroem o terreno**, seja qual for a opção.
- Cada Portão pede um nível: são dez normais, do nível 1 (rank E) ao 250 (rank S), e cinco
  **vermelhos**, de chefe, do nível 15 ao 350. Depois de entrar num vermelho, todos os
  vermelhos ficam fechados por 5 minutos.

## Maestria e árvore (painel do meio do menu)

A maestria da técnica inata vai de 1 a 100 e sobe usando as habilidades (rápido de propósito):
cada Desmantelar dá 3, cada Cleave 1, o corte de teia 5, a flecha de fogo 10 e o domínio 20;
o nível seguinte pede 10 mais 2 por nível. Cada nível par dá um ponto da árvore (50 no total,
menos do que a árvore toda custa: é preciso escolher).

As habilidades se desbloqueiam pela maestria: Desmantelar desde o início, Cleave na 5, corte
de teia na 12, técnica reversa na 20 (pela árvore), flecha de fogo na 30, domínio na 45.

A árvore sai de um tronco (o círculo de baixo, com a sua maestria) em cinco galhos
coloridos: Desmantelar (vermelho), Cleave (laranja, com o corte de teia saindo dele), a
técnica reversa no meio (verde), flecha de fogo (amarelo) e domínio (roxo). O círculo grande
de cada galho é a habilidade: acesa quando já desbloqueada, cinza com a maestria que falta
quando não. Os nós de cima mostram o nível (2/5, por exemplo) e um anel que enche; os que
podem subir agora pulsam. Os caminhos até o que você já aprendeu ficam acesos. Aponte a
mira num nó para ver o que ele faz e o que falta; clique para subir um nível.

Cada nó tem até 5 níveis de 5% (no máximo 25%, para nenhuma habilidade virar outra):

- **Desmantelar:** alcance, dano, ou menos energia.
- **Cleave:** dano, menos energia, ou menos recarga (a mesma do corte de teia).
- **Corte de teia:** raio.
- **Flecha de fogo:** área, dano, ou menos recarga.
- **Domínio:** raio, duração, ou vida da barreira.

### Corte do mundo

O nó branco no galho do Desmantelar, embaixo dos três nós dele. Pede **maestria 30** e os três
nós do Desmantelar (alcance, dano e menos energia) em **5/5**.

Como fazer, com R segurado:

1. Faça um traço de um lado para o outro (de A para B).
2. Faça um segundo traço que **começa onde o primeiro acabou e volta até onde ele começou**
   (de B para A).
3. Os traços ficam brancos: **continue segurando R por 2 segundos**. Um som e o aviso
   "Corte do mundo pronto" avisam que pode soltar.
4. Solte R.

Sai **um único corte** branco ao longo da linha: 128 blocos de alcance, dano de 4000 vezes a
sua força, atravessa tudo, **ignora qualquer defesa** (o dano passa por armadura, encantos,
resistência e invulnerabilidade) e **quebra barreiras de domínio**. Tem 180 segundos de
recarga. Fora de missões, e com o terreno ligado nas opções, corta o terreno
também (é um plano enorme: pode pesar). Se você soltar antes de ficar pronto, não sai nada
(nem os dois cortes normais): o aviso pede para segurar.

### Técnica reversa (tecla H)

Três estágios na árvore, de 2 pontos cada. Segure **H** para se curar gastando energia; ao
soltar começa uma recarga de 15 segundos.

| Estágio | Cura | Energia por ponto de vida |
|---|---|---|
| 1 | 1 coração por segundo | 8 |
| 2 | 1,5 coração por segundo | 6 |
| 3 | 2 corações por segundo | 4 |

No terceiro estágio, **dois toques rápidos em H** ligam (ou desligam) o modo automático: ao
perder vida você se cura sozinho até encher, sem recarga, na metade da velocidade do estágio
e gastando 5 de energia por ponto de vida.

## Configurações (painel CONFIG do menu, ou Opções > Mods > Jujutsu Horizon)

- **Destruição de blocos**: SIM / NÃO. Vale para o Sukuna e para o Gojo. Com SIM, os cortes abrem rasgos e as técnicas destrutivas abrem crateras (do Gojo, todas menos o Azul comum). Com NÃO, nenhuma técnica mexe no terreno; os domínios fechados continuam limpando a área, e tudo volta no fim.
  Bedrock, água, lava e portais não são cortados.
- **Efeitos da explosão**: COMPLETOS (a Nuke do Orbital Laser inteira), REDUZIDOS (o modo
  otimizado do próprio Orbital, com a fumaça desenhada em metade da largura e da altura) ou
  DESLIGADOS (nenhum efeito: só o terreno some). Vale só para a flecha de fogo.
- Dentro de um Portão, o terreno não é cortado seja qual for a opção.

O dano por corte (14) e o alcance (48 blocos) não são mais opções: valem sempre esses. Um
valor que uma versão antiga tenha gravado é ignorado.

## Como compilar

O launcher compila o mod com o Rust dele; não precisa ter Rust instalado. No PowerShell:

```powershell
& "$env:LOCALAPPDATA\Programs\Horizon Launcher\horizon-launcher.exe" native-build "CAMINHO\DA\PASTA\sukuna" --trust-native-code
```

Troque o caminho pelo da pasta onde está o `horizon-mixin.toml`. Se tiver mais de uma instância,
acrescente `--instance "Nome da instância"`.

A primeira compilação baixa o Rust e o código do Horizon e leva de 20 a 30 minutos. As seguintes
levam poucos minutos. Para testar mudanças mais rápido, rode antes
`horizon-launcher settings native-profile dev` (o jogo fica mais lento; volte com `performance`).

`--trust-native-code` declara que você confia no código: um mod nativo roda com os seus
direitos no computador, sem sandbox. O código está em `api/src/` e os shaders em `api/data/`.

## Se a compilação falhar

O mod fixa o Nightly em que foi escrito e checado, o de 13/11/2026 (`[compatibility]` no
`horizon-mixin.toml`), e o launcher compila nele seja qual for o Nightly que a sua instância
joga. Isso é preciso com o launcher 0.2.69: os Nightlies mais novos (o de 22/11, por exemplo)
mudaram as pastas do código do Horizon, e o 0.2.69 não consegue montá-los ("failed to read
... crates\horizon-mods\Cargo.toml").

## O que cada arquivo faz

- `horizon-mixin.toml`: identidade do mod.
- `api/src/lib.rs`: teclas, o desenho dos traços, o voo dos cortes, dano e terreno.
- `api/src/gfx.rs`: monta os retângulos de cada quadro e o passe de desenho (wgpu).
- `api/src/cleave.rs`: o Cleave: agarrar, segurar e retalhar.
- `api/src/whirl.rs`: o redemoinho do Cleave (G olhando para cima).
- `api/src/combat.rs`: o soco, o combo, a defesa e os aparos.
- `api/src/vows.rs`: os pactos vinculativos.
- `api/src/shop.rs`: a loja: os itens, os preços e os pontos de cada Portão.
- `api/src/power.rs`: a barra de energia e o desenho dela no HUD.
- `api/src/fuga.rs`: a flecha de fogo: carga, voo, explosão e cratera.
- `api/src/flame.rs`, `api/data/flame.wgsl`: o desenho da flecha de fogo e a formação dela
  enquanto carrega.
- `api/src/rig.rs`: o corpo do jogador montado em código, com cotovelos e joelhos, e a
  cinemática inversa que dobra cada membro até a mão ou o pé chegar onde a pose quer.
- `api/src/anim.rs`: as poses de cada técnica e como elas entram e saem.
- `api/src/limbs.rs`: o corpo no jogo: o desenho em terceira pessoa, os braços em primeira,
  a mão do jogo escondida e a câmera do pouso.
- `api/src/floor.rs`: o piso do domínio fechado: guarda o chão, põe o piso e devolve o chão.
- `api/src/aura.rs`: a aura de energia (desenhada pelo passe do `gojo.wgsl`, como uma casca em volta do corpo).
- `api/src/progress.rs`: nível, XP, status e o que cada um vale; gastos e recargas.
- `api/src/rpg.rs`: a energia, a experiência por abate, os atributos, o pulo e as barras.
- `api/src/menu.rs`: o que cada painel do menu mostra (status, config, árvore, Portões,
  loja) e os cliques.
- `api/src/holo.rs`, `api/data/holo.wgsl`: os painéis holográficos: onde ficam no mundo, o
  raio da mira, as formas (retângulos, anéis, linhas, curvas, letras) e o passe de desenho.
- `api/data/font.bin`: a fonte Inter SemiBold como atlas de campo de distância (OFL-1.1,
  ver NOTICE).
- `api/data/nuke.wgsl` tem também o modo otimizado do Orbital Laser (fumaça em meio
  tamanho), usado com os efeitos em REDUZIDOS.
- `api/src/gates.rs`: os Portões: a lista, a barreira, as ondas e as recompensas.
- `api/src/domain.rs`: a expansão de domínio: concentração, barreira, cortes e terreno.
- `api/data/domain.wgsl`: o shader da barreira e do domínio aberto (e da barreira amarela
  dos Portões).
- `api/src/shrine.rs`, `api/data/shrine.wgsl`: o santuário, desenhado como malha.
- `api/data/shrine.bin`: o modelo do santuário, já convertido (malha e texturas).
- `api/src/wounds.rs`: as feridas, guardadas no referencial do corpo de cada mob.
- `api/data/slash.wgsl`: o shader do brilho e do rastro.
- `api/data/wound.wgsl`: o shader das feridas (pele, músculo, osso e sangue).
- `api/src/nuke.rs`: o quadro e a tabela que o shader da Nuke lê (relevo, vento, destroços).
- `api/data/nuke.wgsl`: o shader da Nuke do Orbital Laser, com uma linha mudada.
- `api/data/blast.wgsl`: o shader antigo da explosão, usado quando o da Nuke não pode.
- `NOTICE`: o crédito e a licença do Orbital Laser.
- `api/src/played.rs`: testes que jogam o mod contra um jogo de mentira.
- `api/src/preview.rs`: só nos testes: as poses desenhadas com uma pele de teste, e a flecha
  de fogo por cima, para ver as animações sem abrir o jogo.
- `api/src/math.rs`: vetores e o teste de corte contra a caixa de um mob.
- `api/src/gojo.rs`: o Azul e o Azul V1: as fases no relógio, a mola, o gasto, o puxão e os
  blocos arrancados no servidor.
- `api/src/red.rs`: o Vermelho e o Vermelho V1: a carga, o voo, a explosão, o empurrão e a
  cratera no servidor (as explosões do Roxo também passam por aqui).
- `api/src/purple.rs`: o Roxo, o Roxo V1 e o Roxo 200%: o gesto, a carga e a barra, o voo,
  a cena, a fusão e o caminho apagado no servidor.
- `api/src/mugen.rs`: o Mugen: ligar, o gasto, as dobras, o freio no servidor e os golpes que
  não chegam.
- `api/src/void.rs`: o Vazio Ilimitado: a concentração, o tempo e o gasto, as ondulações e a
  paralisia no servidor (o piso e o sumiço dos blocos são o `floor.rs`).
- `api/src/sphere.rs`, `api/data/gojo.wgsl`: o passe das esferas do Gojo, com a lente do
  buraco negro do Orbital Laser (ver NOTICE).

# Design

## Context

Ver `proposal.md` — Why/What Changes. Estado atual do risco em `docs/architecture/overview.md`, seção 6 (texto vigente antes desta change): *"Risco não previsto originalmente, registrado sem decisão fechada: o HotSpot 'de fábrica' também depende de carregamento dinâmico de bibliotecas compartilhadas (`libjvm.so` carregando `libjava.so` via `dlopen`/linker dinâmico, e JNI carregando bibliotecas nativas de aplicações) [...]. Candidato mais promissor para avaliação na change do item 8 do roadmap: build totalmente estático do JDK [...]; alternativa, caso se prove inviável: implementar um linker dinâmico de ELF mínimo no kernel. Nenhuma das duas é decidida nesta etapa."*

Pesquisa feita para fechar essa decisão (ver "Fontes consultadas" ao final — não há como compilar/rodar o HotSpot ou o OpenJDK de verdade neste ambiente):

- **Estado real do build estático do OpenJDK ("Hermetic Java"/Leyden, bug guarda-chuva `JDK-8303796`, branch experimental `hermetic-java-runtime`)**: a flag `--with-static-java=yes` já produz um executável `javastatic` linkado estaticamente contra `libjvm.a`, mas o escopo da primeira entrega é deliberadamente limitado — **só o launcher headless**, não a JVM completa com todos os recursos (ex.: o *Serviceability Agent* é um risco explícito de quebra).
- **A limitação central, que inviabiliza esta rota para o Java OS especificamente**: mesmo um JDK "estático" continua precisando de `dlopen` em tempo de execução para certos componentes (ex.: módulos JNI carregados por aplicações, ou partes do próprio runtime carregadas sob demanda) — a glibc não permite `dlopen()` a partir de um buffer em memória, então a solução adotada pelos próprios autores do Leyden é um *workaround* via `dlopen("/proc/self/fd/" + fd)`, que depende de uma API específica do Linux (`/proc/self/fd`, um sistema de arquivos virtual que expõe os descritores de arquivo abertos do processo). **O Java OS não tem, e não há plano de ter, nada equivalente a `/proc`** — essa solução não é portável para cá.
- Os próprios autores do Leyden registram como risco em aberto que quase todos os símbolos do HotSpot são exportados como globais no build estático ("não é razoável fazer isso num build estático" — texto do próprio bug), o que sugere que o esforço de "estaticizar" de verdade está longe de maduro mesmo para quem o está construindo.
- Nenhuma fonte de 2025 confirma que essas dependências residuais de carregamento dinâmico foram eliminadas, nem que o build estático cobre além do launcher headless experimental.
- **GraalVM Native Image não é uma alternativa válida**: compila uma aplicação Java *específica e conhecida em tempo de compilação* sob *closed-world assumption* (sem carregamento dinâmico de classe em tempo de execução, sem reflection sem configuração explícita) num binário standalone por aplicação. Isso é incompatível com a visão do próprio projeto (`first-prompt.md`, `docs/architecture/overview.md` seções 9-11: Java OS Platform e Gaming Platform carregando JARs/aplicações de terceiros arbitrários em tempo de execução, não apenas uma aplicação fixa compilada antecipadamente).
- **Nenhum precedente direto** foi encontrado de HotSpot (ou JVM derivada) portada para um kernel hobby/próprio fora da família Linux/BSD/Windows. O único achado próximo (um artigo acadêmico de 2008, Okumura/Childers/Mossé) embute uma JVM leve e customizada — não o HotSpot — dentro do kernel do FreeBSD; não é um kernel hobby, e não enfrenta o problema de `dlopen` da mesma forma (a JVM ali é parte do próprio kernel, não um processo de usuário carregando bibliotecas).
- **Achado novo, não coberto por `jvm-posix-compat-strategy`**: o kernel hoje **não tem nenhum carregador de executável ELF**, nem para um binário estaticamente linkado — confirmado por busca no código (`kernel::user_demo`'s próprio comentário: "sem carregador de executáveis ainda"). Todo código de modo usuário existente (`kernel::user_demo`, `kernel::user_posix_demo`, das changes `kernel-syscalls`/`kernel-jvm-posix-syscalls`) é um blob de assembly independente de posição, copiado manualmente para uma página — nunca um arquivo ELF real carregado do disco. Isso significa que, **mesmo que a rota estática fosse viável**, o kernel ainda precisaria aprender a carregar um executável ELF (ler o arquivo via `kernel::fat32`, interpretar o cabeçalho e os *program headers*, mapear os segmentos `PT_LOAD` via `kernel::vmm`/`kernel::paging`, saltar para o ponto de entrada) antes de rodar qualquer coisa parecida com um `javastatic`. Carregar um executável ELF (estático) e fazer o linker dinâmico de bibliotecas compartilhadas (`.so`, símbolos entre módulos, `dlopen`/`dlsym`) são dois problemas relacionados, mas **distintos** — o primeiro é pré-requisito do segundo de qualquer forma, e nenhuma change anterior o cobriu nem o nomeou separadamente do segundo.

## Goals / Non-Goals

**Goals:**
- Fechar a decisão entre build estático do JDK/Leyden e linker ELF dinâmico mínimo no kernel, registrada como Non-Goal explícito em `jvm-posix-compat-strategy`.
- Documentar, com fontes, por que a rota estática é rejeitada nesta etapa (não apenas "ainda incompleta", mas especificamente inviável para o Java OS devido à dependência de `/proc/self/fd`, uma API exclusiva do Linux).
- Registrar o achado de que um carregador de executável ELF estático é um pré-requisito separado e mais imediato do linker dinâmico, e nomeá-lo explicitamente como a próxima sub-change concreta do item 8 do roadmap.
- Delimitar a fronteira entre esta change (decisão, sem código) e as próximas sub-changes de implementação.

**Non-Goals (nesta change):**
- Implementar qualquer parte do carregador de ELF estático ou do linker dinâmico — ambos ficam para sub-changes futuras do item 8 do roadmap.
- Especificar o conjunto exato de relocações ELF (`R_X86_64_*`), o formato exato da tabela de símbolos dinâmicos, ou qualquer outro detalhe de implementação do linker dinâmico — são decisões da change que o implementa, não desta.
- Decidir qual distribuição/versão específica do OpenJDK será portada, ou o processo de *bootstrap* de compilação cruzada do OpenJDK para o Java OS — decisões da change de porte do HotSpot em si (mais à frente no item 8), não desta.
- Reavaliar a decisão já tomada em `jvm-posix-compat-strategy` (camada de compatibilidade POSIX mínima) — permanece válida e não é revisitada aqui.

## Decisions

### Build estático do JDK/Leyden é rejeitado para esta etapa
Não por ser "só" experimental (isso já era sabido em `jvm-posix-compat-strategy`), mas por uma razão mais específica e decisiva: a solução que o próprio projeto Leyden usa para contornar a limitação de `dlopen()` a partir de memória da glibc (`dlopen("/proc/self/fd/" + fd)`) depende de uma API exclusiva do Linux (`/proc`) que o Java OS não tem e não planeja ter. Mesmo que o esforço de build estático amadurecesse completamente, portar essa técnica específica para o Java OS exigiria praticamente reconstruir uma API `/proc`-like só para viabilizá-la — não é uma economia de esforço real frente a implementar um linker dinâmico de ELF do zero, e ainda carregaria o risco adicional de depender de uma linha de trabalho externa (Leyden) que não está sob o controle deste projeto e pode mudar de direção.

**Alternativa descartada**: esperar o amadurecimento do Leyden antes de decidir. Rejeitada porque não há cronograma público, e o inventário de primitivas da JVM (`jvm-posix-compat-strategy`) já está quase todo entregue — manter esta decisão em aberto bloquearia desnecessariamente o planejamento das próximas sub-changes do item 8.

### Linker ELF dinâmico mínimo no kernel é a rota adotada
Decisão: a sub-change (ou sub-changes) futura que implementa isso ficará inteiramente sob controle do próprio projeto, sem depender de nenhuma infraestrutura externa experimental. O escopo exato (quais tipos de relocação suportar, se cobre JNI de aplicações de terceiros desde o início ou só `libjvm`→`libjava`, etc.) é decisão da change que o implementa — não desta.

### Carregador de executável ELF estático é um pré-requisito separado, e deve ser a próxima sub-change
Independentemente da rota de linkagem escolhida para as bibliotecas do JDK, o kernel precisa, primeiro, saber executar **um único binário ELF já linkado** (estático ou com suas dependências dinâmicas resolvidas depois) — isso não existe hoje em nenhuma forma. Esse carregador é um escopo natural e imediato para uma sub-change própria, menor e mais simples que o linker dinâmico completo: ler o arquivo via `kernel::fat32` (já pronto), interpretar o cabeçalho ELF64 e os *program headers*, mapear os segmentos `PT_LOAD` com as permissões corretas via `kernel::vmm`/`kernel::paging` (já prontos), e transferir a execução para o ponto de entrada — sem nenhuma resolução de símbolo entre módulos nem `dlopen`/`dlsym` ainda. O linker dinâmico (`.so`, símbolos, relocações, `dlopen`/`dlsym`) fica para uma sub-change posterior a essa, que pode reaproveitar o parsing de cabeçalho ELF já escrito para o carregador estático.

## Risks / Trade-offs

- **[Risco] Implementar um linker ELF dinâmico mínimo é um esforço de engenharia substancial, comparável em escopo aos maiores já feitos neste projeto** → Aceito deliberadamente: é a única rota inteiramente sob controle do projeto, sem depender de uma linha de trabalho externa experimental e com uma incompatibilidade específica (`/proc`) com o Java OS. Mitigado pela divisão em duas sub-changes (carregador estático primeiro, linker dinâmico depois), cada uma menor e verificável isoladamente, mesma disciplina incremental já usada em todo o item 8.
- **[Risco] O escopo exato do linker dinâmico (quantos tipos de relocação, cobertura de JNI de terceiros) ainda não está decidido** → Deliberadamente deixado para a change que o implementa (ver Non-Goals); decidir agora, sem o carregador estático já funcionando, seria prematuro.
- **[Trade-off] Esta decisão depende de uma avaliação externa (estado do Leyden) que pode mudar no futuro** → Aceito: a decisão é sobre a estratégia para ESTA etapa do roadmap, não uma rejeição permanente; se o Leyden eventualmente resolver a dependência de `/proc` de forma portável, isso pode ser revisitado, mas não há motivo para esperar por isso agora.

## Migration Plan

Não aplicável — esta change não move nenhum estado nem altera nenhum código. "Deploy" desta change é a atualização de `docs/architecture/overview.md` (seção 6 com a decisão tomada; item 8 do roadmap com a próxima sub-change nomeada). Nenhum build/teste automatizado é afetado.

## Fontes consultadas

Pesquisa feita via busca na web (listas de e-mail do próprio projeto OpenJDK, sistema de *bugs* público do OpenJDK, e documentação/anúncios públicos sobre GraalVM Native Image), já que não há como compilar/rodar o HotSpot ou o OpenJDK de verdade neste ambiente de desenvolvimento:

- Discussões arquivadas nas listas `leyden-dev`/`build-dev` do OpenJDK sobre o esforço "Hermetic Java" e a flag `--with-static-java=yes` (launcher `javastatic` estático contra `libjvm.a`, escopo inicial limitado a headless).
- Bug guarda-chuva `JDK-8303796` e bug relacionado `JDK-8350450` (`bugs.openjdk.org`), incluindo o registro, pelos próprios autores, da limitação de `dlopen()` a partir de memória na glibc e do *workaround* via `dlopen("/proc/self/fd/" + fd)`, e do risco de símbolos globais demais para um build estático.
- Branch/linha de trabalho experimental `hermetic-java-runtime` do projeto Leyden.
- Documentação pública sobre GraalVM Native Image e seu modelo de *closed-world assumption*, para confirmar que não é um substituto funcional de uma JVM geral capaz de carregar JARs/classes arbitrários em tempo de execução.
- Busca geral por precedentes de JVM/HotSpot portados para kernels hobby ou sistemas operacionais próprios não-Linux/BSD/Windows (sem achado direto reaproveitável, além de um artigo acadêmico de 2008 sobre uma JVM customizada leve embutida no kernel do FreeBSD, não comparável em arquitetura nem em escopo).

Estas fontes refletem o estado do projeto Leyden/OpenJDK em 2025; a situação pode mudar, mas nenhuma indicação encontrada sugere uma resolução iminente da dependência específica de `/proc/self/fd` que motiva a rejeição desta rota para o Java OS.

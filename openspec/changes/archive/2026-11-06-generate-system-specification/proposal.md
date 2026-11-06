# Proposal

## Why

O projeto Java OS já acumulou 22 changes arquivadas em `openspec/changes/archive/`,
cada uma com seu próprio `design.md`, e um documento de síntese
(`docs/architecture/overview.md`) que cobre 15 áreas de arquitetura — mas esse
conhecimento está fragmentado entre dezenas de arquivos, escrito incrementalmente
change a change, sem um único documento que amarre visão de negócio, arquitetura,
regras, dados em disco, interfaces (syscalls/ABI) e lacunas conhecidas em um só
lugar. `CLAUDE.md` já declara `SPECIFICATION.md` como a "única fonte oficial de
especificação" do projeto daqui para frente, mas esse arquivo nunca foi criado —
não há hoje um documento que permita a outro time recriar o sistema do zero sem
ler o código-fonte e as 22 changes arquivadas.

O usuário forneceu um prompt (`Você é um Arquiteto de Software.md`) e um template
de 25 seções (`Specification-Driven Development.pdf`) definindo o padrão de
Specification-Driven Development (SDD) a seguir. Esta change propõe gerar esse
`SPECIFICATION.md` agora, consolidando o código-fonte (fonte da verdade), as specs
arquivadas e `docs/architecture/overview.md` em um documento mestre único.

## What Changes

- Criar `SPECIFICATION.md` na raiz do repositório, documento Markdown único,
  cobrindo o sistema completo: kernel Rust `no_std` bare-metal x86_64 (boot
  UEFI/Limine, memória, processos/threads, syscalls, ELF/linker dinâmico, FAT32,
  rede, POSIX/pthreads) e os componentes de userland de teste (`userland_c`,
  `elf_test_program`, `elf_test_library_base/user`).
- Adaptar o template de 25 seções do PDF (voltado a aplicações web
  CRUD/REST/banco de dados) ao domínio de sistema operacional — mapeando cada
  seção genérica para o equivalente real do projeto (ex.: "APIs" → tabela de
  syscalls; "Banco de Dados" → layout on-disk FAT32/GPT; "Telas" → console de
  framebuffer; "Integrações" → hardware/protocolos de baixo nível) e marcando
  explicitamente como "Não aplicável" as seções sem equivalente (ex.: OAuth2/SSO,
  Kafka/RabbitMQ, dashboards Grafana/Kibana), com a justificativa do porquê.
- Incluir diagramas Mermaid (fluxogramas de boot/execução, diagrama de camadas do
  kernel, diagramas de sequência de syscalls/carregamento ELF) conforme pedido no
  prompt de origem.
- Documentar explicitamente toda ambiguidade encontrada (o próprio arquivo
  `Você é um Arquiteto de Software.md` está incompleto — define em detalhe apenas
  a seção 1, "Visão Geral", e corta no meio de um bloco de exemplo Mermaid, sem
  detalhar as seções 2 em diante) e todo comportamento implícito identificado no
  código-fonte que hoje não está registrado em nenhuma spec arquivada.
- Apontar divergências entre o código-fonte atual e as specs já arquivadas
  (se encontradas), já que `CLAUDE.md` determina que, em caso de conflito,
  qualquer inconsistência deve ser relatada.
- Nenhum código de produção, script de build ou configuração é alterado por esta
  change — é puramente geração de documentação, por isso não há requisitos de
  comportamento de sistema a capturar em `specs/` (ver `skip_specs: true` em
  `.openspec.yaml`).

## Capabilities

### New Capabilities

Nenhuma. Esta change não introduz nem altera comportamento do sistema — apenas
consolida documentação já existente (código-fonte + specs arquivadas +
`docs/architecture/overview.md`) em um novo documento. `.openspec.yaml` desta
change declara `skip_specs: true`.

### Modified Capabilities

Nenhuma.

## Impact

- **Arquivo novo**: `SPECIFICATION.md` (raiz do repositório).
- **Nenhum código, script, configuração de build ou spec arquivada é alterado.**
- **Leitura necessária**: todo `kernel/src/*.rs`, `userland_c/`,
  `elf_test_program/`, `elf_test_library_base/`, `elf_test_library_user/`,
  `xtask/`, `scripts/`, `docs/architecture/overview.md`, `README.md`,
  `KNOW_ISSUES.md`, e as 22 changes em `openspec/changes/archive/` (design.md e
  specs de cada uma) e as 25 capabilities em `openspec/specs/`.
- **Consumidores futuros**: `CLAUDE.md` já instrui que toda dúvida futura sobre
  regras de negócio/comportamento do sistema deve ser resolvida consultando
  `SPECIFICATION.md` como fonte única da verdade — este documento passa a ser
  referência obrigatória para changes futuras.

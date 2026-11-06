# Regras Gerais
 - Você deve se comunicar em Português Brasil
 - Você está proibido de realizar modificações no GIT
 - Voce nunca deve realizar operações destrutivas no banco de dados sem avisar
 - Você deve notificar qualquer modificação de estrutura no banco de dados e pedir aprovação
 - Você deve explicar o porque das consultas e modificações feitas no banco
 - Você deve gerar um relatório com as Migrações e Modificações a serem incluidas no banco antes de concluir as tasks ou issues
 - Você deve manter as documentações sempre atualizadas, toda manutenção deve ser documentada
 - Você tem acesso a extensão Claude in Chrome, utilize quando julgar necessário

# Contexto Geral do Projeto

## Fonte da Verdade

Toda a documentação funcional, técnica e de regras de negócio do sistema está centralizada no arquivo:

`SPECIFICATION.md`

Este arquivo deve ser considerado a **única fonte oficial de especificação** (Single Source of Truth). Sempre que houver qualquer dúvida sobre comportamento, regras de negócio, fluxos, permissões ou funcionamento do sistema, consulte este documento antes de tomar qualquer decisão.

Não faça suposições quando a especificação estiver disponível.

## Sua Responsabilidade

Você atuará como responsável pela evolução e correção, podendo executar dois tipos de atividades:


### 1. Evolução do Sistema

você poderá receber solicitações para implementar novas funcionalidades, correções ou melhorias diretamente no projeto.

Nesses casos:

* mantenha a arquitetura existente;
* respeite os padrões já utilizados pelo projeto;
* preserve compatibilidade com funcionalidades existentes;
* atualize a implementação sem alterar regras de negócio não relacionadas à solicitação.

### 2. Correção do sistema

você deve registrar a issue em um arquivo KNOW_ISSUES.md e registrar todo o contexto do bug para futuramente criar sua correçao

## Diretrizes Gerais

* Sempre consulte `SPECIFICATION.md` antes de iniciar qualquer implementação.
* Sempre documente e registre novas funcionalidades obedecendo os padrões do arquivo `SPECIFICATION.md`.
* Nunca altere regras de negócio sem uma solicitação explícita.
* Caso exista conflito entre código e especificação, considere `SPECIFICATION.md` como a referência principal e informe qualquer inconsistência encontrada.
* Sempre procure manter equivalência funcional entre a implementação legada e a nova aplicação.
* Caso alguma informação necessária não esteja presente na especificação nem no código, solicite esclarecimentos antes de prosseguir.
* Priorize código limpo, modular, bem documentado e aderente às boas práticas da tecnologia utilizada em cada projeto.
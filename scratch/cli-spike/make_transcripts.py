#!/usr/bin/env python3
"""Gera transcripts sintéticos PT-BR diarizados para o spike C1.

Formato de linha espelha o do app: "[MM:SS] Falante: texto"
(frontend/src/hooks/meeting-details/useSummaryGeneration.ts:448-459).
Mistura falantes renomeados (João, Maria) e anônimos (Speaker 2, Speaker 3).
Um marcador de decisão distintivo é colocado NO FINAL para o teste de stdin grande.
"""
import sys

# Blocos de diálogo realistas de uma reunião de planejamento de sprint.
DIALOGO = [
    ("João", "Bom dia pessoal, vamos começar a retro da sprint 14. Quero focar em três coisas: o que travou, os action items abertos e o planejamento do release."),
    ("Maria", "Bom dia. Do meu lado, a integração com o gateway de pagamento ficou pronta, mas ainda falta cobrir os testes de timeout que o João pediu."),
    ("Speaker 2", "Eu fiquei responsável pela parte de observabilidade. Consegui subir os dashboards no Grafana, mas os alertas de latência ainda estão com muito falso positivo."),
    ("João", "Certo. Maria, consegue fechar os testes de timeout até quinta? A gente precisa disso antes do freeze."),
    ("Maria", "Consigo sim, quinta de manhã eu subo o PR. Só vou precisar de review rápido porque toca no fluxo de checkout."),
    ("Speaker 3", "Sobre o checkout, eu notei ontem que a tela de confirmação está lenta em conexões ruins. Vale abrir um card separado pra isso."),
    ("João", "Boa observação. Speaker 3, abre o card e coloca como prioridade média por enquanto. A gente reavalia na próxima."),
    ("Speaker 2", "Voltando aos alertas: eu proponho aumentar a janela de agregação de 1 pra 5 minutos. Isso deve matar a maioria dos falsos positivos sem perder incidente real."),
    ("Maria", "Concordo, 5 minutos me parece razoável. A gente teve um incidente real mês passado e ele durou uns 12 minutos, então 5 ainda pega."),
    ("João", "Fechado, muda pra 5 minutos. Speaker 2, você toca essa mudança ainda hoje?"),
    ("Speaker 2", "Toco sim, é só ajustar a config do Prometheus e reiniciar o alertmanager. Deixo pronto até o fim do dia."),
    ("Speaker 3", "Uma dúvida: a gente vai manter o deploy manual ou já migra pro pipeline automático nessa release?"),
    ("João", "Boa pergunta. Eu quero migrar, mas com portão de aprovação manual antes de produção. Ninguém vai em prod sem um humano apertar o botão."),
    ("Maria", "Isso me deixa mais tranquila. O pipeline automático já roda os testes de integração, né? Se rodar, eu confio."),
    ("Speaker 2", "Roda sim, adicionei a suíte de integração no estágio de CI semana passada. Cobre os caminhos críticos de pagamento e login."),
    ("João", "Perfeito. Então o combinado é: pipeline automático até staging, aprovação manual pra produção."),
    ("Speaker 3", "E sobre rollback? Se der problema em prod, qual o plano?"),
    ("João", "Rollback é redeploy da tag anterior. A gente mantém as últimas três tags sempre prontas. Speaker 2 documenta isso no runbook."),
    ("Speaker 2", "Anotado, coloco no runbook hoje junto com a mudança dos alertas."),
    ("Maria", "Só lembrando que na sprint passada a gente ficou sem documentar o processo de migração de banco e deu confusão. Vamos evitar repetir."),
    ("João", "Verdade. Maria, você assume a doc da migração de banco dessa vez? Curtinho, só os passos e o comando de rollback."),
    ("Maria", "Assumo. Faço junto com o PR dos testes de timeout, aproveito o embalo."),
    ("Speaker 3", "Do lado de UX, a gente recebeu feedback de que os usuários não entendem a mensagem de erro quando o cartão é recusado. Tá muito técnica."),
    ("João", "Concordo, aquela mensagem é péssima. Speaker 3, você consegue propor um texto novo e alinhar com a Maria pra não quebrar a lógica de erro?"),
    ("Speaker 3", "Consigo. Escrevo três variações e trago na próxima pra gente escolher."),
    ("Maria", "Só me manda antes pra eu ver se os códigos de erro batem com o que o gateway retorna. Alguns erros a gente não pode expor por segurança."),
    ("Speaker 2", "Aproveitando segurança: o scan de dependências acusou uma lib com CVE alto. É a lib de parsing de XML. Precisa atualizar."),
    ("João", "Isso é bloqueante pro release. Speaker 2, sobe o bump da lib hoje ainda e roda a suíte completa. Se passar, entra no freeze."),
    ("Speaker 2", "Beleza, priorizo isso antes do runbook então. Segurança primeiro."),
    ("João", "Isso. Alguém tem mais alguma coisa antes da gente decidir o nome e a data do release?"),
    ("Maria", "Do meu lado é só isso. Testes de timeout, doc de migração, e review do texto de erro."),
    ("Speaker 3", "Fechado pra mim também. Card de performance do checkout e as variações de mensagem."),
    ("Speaker 2", "Eu tô com bump da lib, mudança de alertas e runbook. Dá pra fazer tudo hoje e amanhã."),
]

DECISAO_FINAL = [
    ("João", "Então vamos formalizar as decisões finais dessa reunião pra ninguém esquecer."),
    ("João", "DECISAO FINAL 1: o codinome oficial desse release passa a ser 'Jabuticaba-9'. Anotem em todo lugar, PR, tag e changelog."),
    ("João", "DECISAO FINAL 2: a data de deploy em producao fica travada para o dia 30 de setembro, as 22h, com portao de aprovacao manual."),
    ("João", "DECISAO FINAL 3: o responsavel por apertar o botao de deploy em producao sera a Maria, com o Speaker 2 de plantao para rollback."),
    ("Maria", "Anotado. Jabuticaba-9, dia 30 de setembro as 22h, eu no deploy e o Speaker 2 no plantao de rollback."),
    ("Speaker 2", "Confirmado, fico de plantao. Runbook de rollback vai estar pronto muito antes disso."),
    ("João", "Perfeito. Reuniao encerrada, obrigado pessoal."),
]


def fmt(mins, secs, speaker, text):
    return f"[{mins:02d}:{secs:02d}] {speaker}: {text}"


def build(target_chars=None):
    lines = []
    t = 0  # segundos
    body = DIALOGO
    if target_chars:
        # repete o corpo (variando o timestamp) ate passar do alvo, depois anexa a decisao no fim
        while sum(len(l) + 1 for l in lines) < target_chars:
            for speaker, text in body:
                mins, secs = divmod(t, 60)
                lines.append(fmt(mins, secs, speaker, text))
                t += 17
    else:
        for speaker, text in body:
            mins, secs = divmod(t, 60)
            lines.append(fmt(mins, secs, speaker, text))
            t += 23
    # decisao final SEMPRE por ultimo (marcador de fim)
    for speaker, text in DECISAO_FINAL:
        mins, secs = divmod(t, 60)
        lines.append(fmt(mins, secs, speaker, text))
        t += 15
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    normal = build(target_chars=10000)
    with open("transcript_normal.txt", "w") as f:
        f.write(normal)
    large = build(target_chars=110000)
    with open("transcript_large.txt", "w") as f:
        f.write(large)
    print(f"normal: {len(normal)} chars, {normal.count(chr(10))} linhas")
    print(f"large:  {len(large)} chars, {large.count(chr(10))} linhas")

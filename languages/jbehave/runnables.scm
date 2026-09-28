; Runnable detection for JBehave scenarios. This query is purely syntactic and
; location-agnostic: it matches whatever .story buffer is open. @run places the
; run button on the Scenario: keyword, and the scenario title is exposed to the
; run command as $ZED_CUSTOM_NAME. The actual command is supplied per project
; (a custom runnable in Zed settings matched on the tag, or later by the
; jbehave-language-server) — never here.
((scenario
  (scenario_kw) @run
  (title)? @name)
 (#set! tag jbehave-scenario))

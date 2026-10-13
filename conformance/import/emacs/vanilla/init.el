(require 'org)
(setq org-agenda-files
      (list (expand-file-name "tasks.org" (getenv "AGD_FIXTURE_DIR"))))
(setq org-todo-keywords
      '((sequence "TODO(t)" "NEXT(n!)" "WAIT(w@)" "|" "DONE(d!)" "CANCELLED(c@)")))
(setq org-tag-alist '(("work" . ?w) ("home" . ?h)))
(setq org-tag-persistent-alist '(("important" . ?i)))
(setq org-tags-exclude-from-inheritance '("noinherit"))
(setq org-highest-priority ?A
      org-lowest-priority ?D
      org-default-priority ?C
      org-deadline-warning-days 5
      org-agenda-span 'week
      org-agenda-start-day "-1d"
      org-agenda-start-on-weekday 0
      org-agenda-skip-scheduled-if-done t
      org-agenda-skip-deadline-if-done t
      org-agenda-skip-timestamp-if-done nil
      org-agenda-sorting-strategy '(priority-down time-up category-keep)
      org-log-done 'time
      org-log-repeat 'note
      org-log-into-drawer t)
(setq org-capture-templates
      `(("t" "Todo" entry (file+headline ,(expand-file-name "tasks.org" (getenv "AGD_FIXTURE_DIR")) "Inbox") "* TODO %?\n:CREATED: %U")
        ("x" "Dynamic" entry (function ignore) "* TODO %(danger)")))
(setq org-agenda-custom-commands
      '(("w" "Work" tags-todo "+work/TODO|NEXT")
        ("r" "Review" agenda "" ((org-agenda-skip-function '(lambda () nil))))))

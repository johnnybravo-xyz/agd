(require 'org)
(setq org-agenda-files
      (list (expand-file-name "tasks.org" (getenv "AGD_FIXTURE_DIR"))))
(setq org-todo-keywords
      '((sequence "TODO" "NEXT" "PROG" "WAIT" "HOLD" "|" "DONE")))

;; Groups kept in a variable, as large configs do.
(defvar my-day-groups
  '((:name "Parked" :todo ("HOLD") :order 9)
    (:name "Next" :todo "NEXT" :order -1)
    (:name "Overdue" :deadline past)
    (:name "Due today" :deadline today :order 1)
    (:name "Due soon" :deadline future :order 7)
    (:name "Errands" :tag ("errand" "shop") :priority "A" :order 2)))

(defvar my-backlog-groups
  '((:discard (:todo ("HOLD")))
    (:name "Inbox" :not (:todo t))
    (:name "Ready" :and (:todo "TODO" :scheduled nil) :order 1)
    (:name "Blocked" :property "BLOCKED_BY" :order 2)
    (:auto-category t :order 9)))

(setq org-agenda-custom-commands
      '(("d" "Day"
         ((agenda "" ((org-agenda-span 1)
                      (org-super-agenda-groups my-day-groups)))
          (alltodo "" ((org-agenda-overriding-header "Backlog")
                       (org-super-agenda-groups my-backlog-groups)))))
        ("w" "Week" agenda "" ((org-super-agenda-groups nil)))
        ("i" "Inline" alltodo ""
         ((org-super-agenda-groups
           '((:name "Mine" :todo "NEXT" :face (:underline t))
             (:auto-parent t)))))
        ("n" "Neglected" alltodo ""
         ((org-super-agenda-groups
           '((:name "Stale" :pred my-stale-p)
             (:name "Rest" :anything t)))))))

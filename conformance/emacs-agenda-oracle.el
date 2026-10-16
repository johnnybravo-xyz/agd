(require 'cl-lib)
(require 'org)
(require 'org-agenda)

(let* ((case-dir (or (getenv "AGD_CASE_DIR")
                     (error "AGD_CASE_DIR is required")))
       (from (or (getenv "AGD_FROM")
                 (error "AGD_FROM is required")))
       (today (or (getenv "AGD_TODAY")
                  (error "AGD_TODAY is required")))
       (date-parts (mapcar #'string-to-number (split-string today "-")))
       (fixed-time (encode-time 0 0 12
                                (nth 2 date-parts)
                                (nth 1 date-parts)
                                (nth 0 date-parts)
                                t))
       (days (string-to-number (or (getenv "AGD_DAYS") "1"))))
  (setq org-agenda-files (directory-files case-dir t "\\.org\\'")
        org-agenda-start-day from
        org-agenda-start-on-weekday nil
        org-agenda-span days
        org-agenda-use-time-grid nil
        org-agenda-include-diary nil
        org-agenda-skip-scheduled-if-done
        (equal (getenv "AGD_SKIP_SCHEDULED_IF_DONE") "true")
        org-agenda-skip-deadline-if-done
        (equal (getenv "AGD_SKIP_DEADLINE_IF_DONE") "true")
        org-agenda-skip-timestamp-if-done
        (equal (getenv "AGD_SKIP_TIMESTAMP_IF_DONE") "true"))
  ;; A named span such as month, whose header names it; AGD_DAYS alone
  ;; gives a day count (1 and 7 still print as Day and Week).
  (when (getenv "AGD_SPAN")
    (setq org-agenda-span (intern (getenv "AGD_SPAN"))))
  (when (equal (getenv "AGD_LOG") "true")
    (setq org-agenda-start-with-log-mode t))
  (when (getenv "AGD_SORTING")
    (setq org-agenda-sorting-strategy
          `((agenda ,@(mapcar #'intern (split-string (getenv "AGD_SORTING")))))))
  ;; A case with AGD_MATCH runs that tags or tags-todo match as a custom
  ;; command instead of the agenda.
  (when (getenv "AGD_MATCH")
    (setq org-agenda-custom-commands
          `(("x" "match" ,(intern (or (getenv "AGD_MATCH_KIND") "tags"))
             ,(getenv "AGD_MATCH")))))
  ;; org-today reads the clock through the time-since primitive, and the
  ;; tags matcher's <today>/<now> through float-time and decode-time; a
  ;; current-time override reaches none of these C primitives, so freeze
  ;; each one's "now".
  (let ((float-time-now (symbol-function 'float-time))
        (decode-time-now (symbol-function 'decode-time)))
    (cl-letf (((symbol-function 'current-time) (lambda () fixed-time))
              ((symbol-function 'org-today) (lambda () (time-to-days fixed-time)))
              ((symbol-function 'float-time)
               (lambda (&optional time) (funcall float-time-now (or time fixed-time))))
              ((symbol-function 'decode-time)
               (lambda (&optional time zone form)
                 (funcall decode-time-now (or time fixed-time) zone form))))
      ;; The CSV rows, a form-feed line, then the text agenda.
      (let ((key (if (getenv "AGD_MATCH") "x" "a")))
        (org-batch-agenda-csv key)
        (princ "\f\n")
        (org-batch-agenda key)))))
